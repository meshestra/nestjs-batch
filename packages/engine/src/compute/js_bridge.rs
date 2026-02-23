use std::sync::{Arc, Mutex};

use napi::{
    bindgen_prelude::*,
    threadsafe_function::{
        ErrorStrategy, ThreadsafeFunction, ThreadSafeCallContext, ThreadsafeFunctionCallMode,
    },
    JsFunction,
};
use serde_json::Value;
use thiserror::Error;

// ─────────────────────────────────────────────────────────────────────────────
// 에러 타입
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum JsBridgeError {
    #[error("JS Processor 타임아웃 ({0}ms)")]
    Timeout(u64),

    #[error("JS Processor가 에러를 반환함: {0}")]
    JsError(String),

    #[error("JSON 파싱 실패: {0}")]
    ParseFailed(String),

    #[error("reply 채널 끊김")]
    ChannelDropped,
}

// ─────────────────────────────────────────────────────────────────────────────
// 내부 채널 타입
//
// chunk_executor.rs와 동일한 검증된 패턴:
//   - oneshot 채널로 napi::Result<String> 전달
//   - JsBridgeError는 NAPI 경계 밖에서만 사용
//   - NAPI 내부는 항상 napi::Result<String> 유지
// ─────────────────────────────────────────────────────────────────────────────

type ReplyTx = tokio::sync::oneshot::Sender<napi::Result<String>>;
type ProcessorTsfn = ThreadsafeFunction<
    (String, Arc<Mutex<Option<ReplyTx>>>),
    ErrorStrategy::CalleeHandled,
>;

// ─────────────────────────────────────────────────────────────────────────────
// JsProcessorDirect
//
// TS의 `ItemProcessor.process(items)` 콜백을 Tokio async 컨텍스트에서
// 안전하게 호출하는 브릿지.
//
// 데이터 흐름:
//   Rust (Tokio) → [items JSON] → JS event loop → [processed JSON] → Rust
//
// 콜백 규약 (NAPI CalleeHandled):
//   (_err: null, itemsJson: string, reply: (json: string) => void) => void
//   - _err: NAPI가 자동으로 null 삽입
//   - itemsJson: 아이템 배열 JSON 문자열
//   - reply: 처리 완료 후 정확히 1회 호출
//     - 성공: reply(JSON.stringify(processedItems))
//     - 실패: reply("__ERROR__:message")
// ─────────────────────────────────────────────────────────────────────────────

pub struct JsProcessorDirect {
    tsfn: ProcessorTsfn,
    timeout_ms: u64,
}

impl JsProcessorDirect {
    /// JS Processor 콜백 함수로부터 생성한다.
    ///
    /// `processor_fn`: TS의 `(_err, itemsJson, reply) => void` 콜백
    /// `timeout_ms`: JS 응답 대기 타임아웃 (기본 30_000)
    pub fn new(processor_fn: JsFunction, timeout_ms: u64) -> napi::Result<Self> {
        let tsfn: ProcessorTsfn = processor_fn.create_threadsafe_function(
            0,
            |ctx: ThreadSafeCallContext<(String, Arc<Mutex<Option<ReplyTx>>>)>| {
                let (items_json, reply_tx_arc) = ctx.value;
                let env = ctx.env;

                // arg0: itemsJson 문자열
                let arg0 = env.create_string(&items_json)?.into_unknown();

                // arg1: reply(json) — oneshot sender를 1회 소비
                let reply_fn: napi::JsFunction =
                    env.create_function_from_closure("reply", move |ctx| {
                        let json: String = ctx.get::<String>(0).unwrap_or_default();
                        if let Ok(mut guard) = reply_tx_arc.lock() {
                            if let Some(tx) = guard.take() {
                                let result = if json.starts_with("__ERROR__:") {
                                    Err(napi::Error::from_reason(json[10..].to_string()))
                                } else {
                                    Ok(json)
                                };
                                let _ = tx.send(result);
                            }
                        }
                        ctx.env.get_undefined()
                    })?;

                Ok(vec![arg0, reply_fn.into_unknown()])
            },
        )?;

        Ok(Self { tsfn, timeout_ms })
    }

    /// chunk 아이템 배열을 JS Processor에 전달하고 처리 결과를 기다린다.
    ///
    /// - items → JSON 직렬화 → JS 호출 → 처리 완료 대기 → JSON 역직렬화
    /// - 직렬화 2회 발생 (JS Processor 사용 시 불가피)
    pub async fn process(
        &self,
        items: &[Value],
    ) -> std::result::Result<Vec<Value>, JsBridgeError> {
        // 아이템 배열 → JSON 문자열 (? 대신 match로 NAPI Result 타입 충돌 회피)
        let items_json = match serde_json::to_string(items) {
            Ok(s) => s,
            Err(e) => return Err(JsBridgeError::ParseFailed(e.to_string())),
        };

        // oneshot 채널: JS reply → Rust
        let (tx, rx) = tokio::sync::oneshot::channel::<napi::Result<String>>();
        let reply_tx_arc = Arc::new(Mutex::new(Some(tx)));

        // JS event loop에 비동기 호출 예약
        self.tsfn.call(
            Ok((items_json, reply_tx_arc)),
            ThreadsafeFunctionCallMode::NonBlocking,
        );

        // JS 완료 대기 (타임아웃 포함)
        let result_json = match tokio::time::timeout(
            std::time::Duration::from_millis(self.timeout_ms),
            rx,
        )
        .await
        {
            Ok(Ok(Ok(json))) => json,
            Ok(Ok(Err(e)))   => return Err(JsBridgeError::JsError(e.to_string())),
            Ok(Err(_))       => return Err(JsBridgeError::ChannelDropped),
            Err(_)           => return Err(JsBridgeError::Timeout(self.timeout_ms)),
        };

        // 처리 결과 JSON → Vec<Value>
        match serde_json::from_str::<Vec<Value>>(&result_json) {
            Ok(v) => Ok(v),
            Err(e) => Err(JsBridgeError::ParseFailed(e.to_string())),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ProcessorKind
//
// chunk_executor가 세 가지 경로를 단일 타입으로 다루기 위한 열거형.
//
//   Native   → rayon 병렬 계산 (CPU bound, 직렬화 0회)
//   JsBridge → JS 콜백 (async I/O 가능, 직렬화 2회)
//   None     → Reader → Writer 직통 (직렬화 0회)
// ─────────────────────────────────────────────────────────────────────────────

pub enum ProcessorKind {
    Native(crate::compute::native::NativeProcessor),
    JsBridge(JsProcessorDirect),
    None,
}

impl ProcessorKind {
    /// 아이템 배열을 처리하고 변환된 배열을 반환한다.
    pub async fn process(
        &self,
        items: Vec<Value>,
    ) -> std::result::Result<Vec<Value>, ProcessorError> {
        match self {
            ProcessorKind::Native(p) => match p.process_async(items).await {
                Ok(v) => Ok(v),
                Err(e) => Err(ProcessorError::Native(e)),
            },
            ProcessorKind::JsBridge(p) => match p.process(&items).await {
                Ok(v) => Ok(v),
                Err(e) => Err(ProcessorError::JsBridge(e)),
            },
            ProcessorKind::None => Ok(items),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProcessorError {
    #[error("Native Processor 실패: {0}")]
    Native(crate::compute::native::NativeComputeError),

    #[error("JS Bridge Processor 실패: {0}")]
    JsBridge(JsBridgeError),
}
