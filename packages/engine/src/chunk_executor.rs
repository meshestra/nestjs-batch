use std::sync::{Arc, Mutex};

use napi::{
    bindgen_prelude::*,
    threadsafe_function::{
        ErrorStrategy, ThreadsafeFunction, ThreadSafeCallContext,
        ThreadsafeFunctionCallMode,
    },
    JsFunction,
};
use napi_derive::napi;
use serde_json::Value;
use tracing::{debug, error, info, warn};

use crate::compute::{ProcessorKind, JsProcessorDirect};
use crate::io::{DbReader, DbWriter, QueryDef, WriteQuery};
use crate::job_execution::{JobExecution, JobStatus, StepExecution};

// ─────────────────────────────────────────────────────────────────────────────
// 핵심 설계
//
// 문제: Rust worker thread (Task::compute) 가 blocking recv 를 하는 동안
//       JS event loop 도 같이 멈추므로 deadlock 이 발생한다.
//
// 해결: execute() 를 async Task 대신 JsDeferred + Tokio spawn 으로 구현한다.
//
//   1. execute() 가 호출 스레드(JS main thread) 에서 JsDeferred 를 생성한다.
//   2. Tokio task 를 spawn 한다. 이 task 는 JS event loop 와 독립적이다.
//   3. Tokio task 에서 JS 콜백을 call(NonBlocking) 으로 호출한다.
//   4. JS 콜백은 비동기 완료 후 reply_fn (NAPI 클로저) 을 호출한다.
//      reply_fn 은 tokio::sync::oneshot 채널로 결과를 전송한다.
//   5. Tokio task 는 oneshot receiver.await 로 결과를 기다린다.
//   6. 모든 Step 이 완료되면 JsDeferred.resolve(result) 를 호출한다.
// ─────────────────────────────────────────────────────────────────────────────

// JS → Rust 결과 채널 타입
type ReplyTx = tokio::sync::oneshot::Sender<napi::Result<String>>;

// Rust → JS 호출 Tsfn 타입 (payload: CallPayload)
type CallTsfn = ThreadsafeFunction<CallPayload, ErrorStrategy::CalleeHandled>;

// execute_native writer_query_fn 전용 타입
// payload: (item JSON Value, reply sender)
type WriterReplyTx = std::sync::mpsc::Sender<napi::Result<String>>;
type WriterQueryTsfn = ThreadsafeFunction<
    (Value, Arc<Mutex<Option<WriterReplyTx>>>),
    ErrorStrategy::CalleeHandled,
>;

// payload: Rust → JS 로 보내는 데이터
struct CallPayload {
    step: u8,          // 0=reader, 1=processor, 2=writer
    offset: u32,       // reader 전용
    items_json: String,
    /// JS 콜백 완료 후 reply_fn 이 이 sender 로 결과를 전송
    reply_tx: Arc<Mutex<Option<ReplyTx>>>,
}

// ─────────────────────────────────────────────────────────────────────────────
// ChunkExecutorOptions
// ─────────────────────────────────────────────────────────────────────────────

#[napi(object)]
#[derive(Debug, Clone)]
pub struct ChunkExecutorOptions {
    pub job_name: String,
    pub step_name: String,
    pub chunk_size: u32,
    pub skip_limit: Option<u32>,
    pub retry_limit: Option<u32>,
    pub enable_logging: Option<bool>,
}

// ─────────────────────────────────────────────────────────────────────────────
// StopSignal
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct StopSignal(Arc<Mutex<bool>>);

impl StopSignal {
    pub fn new() -> Self { Self(Arc::new(Mutex::new(false))) }
    pub fn stop(&self)   { if let Ok(mut f) = self.0.lock() { *f = true; } }
    pub fn is_stopped(&self) -> bool { self.0.lock().map(|f| *f).unwrap_or(false) }
    pub fn reset(&self)  { if let Ok(mut f) = self.0.lock() { *f = false; } }
}

// ─────────────────────────────────────────────────────────────────────────────
// ChunkExecutor
// ─────────────────────────────────────────────────────────────────────────────

#[napi]
pub struct ChunkExecutor {
    options: ChunkExecutorOptions,
    stop_signal: StopSignal,
}

#[napi]
impl ChunkExecutor {
    #[napi(constructor)]
    pub fn new(options: ChunkExecutorOptions) -> Self {
        if options.enable_logging.unwrap_or(true) {
            let _ = tracing_subscriber::fmt()
                .with_env_filter("nestjs_batch_engine=debug")
                .try_init();
        }
        info!(job=%options.job_name, step=%options.step_name, "ChunkExecutor 초기화");
        Self { options, stop_signal: StopSignal::new() }
    }

    #[napi(getter)] pub fn job_name(&self)   -> String { self.options.job_name.clone() }
    #[napi(getter)] pub fn step_name(&self)  -> String { self.options.step_name.clone() }
    #[napi(getter)] pub fn chunk_size(&self) -> u32    { self.options.chunk_size }

    #[napi]
    pub fn request_stop(&self) {
        warn!(job=%self.options.job_name, "외부 중단 신호 수신");
        self.stop_signal.stop();
    }

    /// Native DB I/O 배치를 실행하고 JS Promise<JobExecution> 을 반환한다.
    ///
    /// Reader/Writer가 Rust sqlx로 DB를 직접 처리하므로 NAPI 경계를 최소화한다.
    ///
    /// - `data_source`     : NativeDataSource (sqlx 커넥션 풀)
    /// - `reader_query`    : SELECT 쿼리 문자열 (LIMIT/OFFSET 자동 부가)
    /// - `writer_query_fn` : `(item: unknown) => { sql: string; params: unknown[] } | null`
    /// - `processor_fn`    : 선택적 JS Processor. 없으면 Reader → Writer 직통 (직렬화 0회)
    #[napi]
    pub fn execute_native(
        &self,
        env: Env,
        data_source: &crate::NativeDataSource,
        reader_query: String,
        writer_query_fn: JsFunction,
        processor_fn: Option<JsFunction>,
    ) -> napi::Result<Object> {
        let pool = Arc::clone(&data_source.pool);

        // DbReader: 정적 쿼리 — Rust가 LIMIT/OFFSET 자동 부가
        let reader = DbReader::new((*pool).clone(), QueryDef::Static(reader_query));

        // DbWriter: writer_query_fn을 ThreadsafeFunction으로 변환
        // JS 함수 `(item) => { sql, params } | null` 을 Rust 클로저로 래핑
        let writer_tsfn: WriterQueryTsfn = writer_query_fn.create_threadsafe_function(
            0,
            |ctx: ThreadSafeCallContext<(Value, Arc<Mutex<Option<WriterReplyTx>>>)>| {
                let (item, reply_tx_arc) = ctx.value;
                let env = ctx.env;

                let item_json = serde_json::to_string(&item)
                    .map_err(|e| napi::Error::from_reason(e.to_string()))?;
                let arg0 = env.create_string(&item_json)?.into_unknown();

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

        let writer_tsfn = Arc::new(writer_tsfn);
        let writer = DbWriter::new((*pool).clone(), move |item: &Value| -> Option<WriteQuery> {
            // ThreadsafeFunction을 동기적으로 호출하기 위해 tokio oneshot 사용
            let tsfn = Arc::clone(&writer_tsfn);
            let item = item.clone();

            // Tokio 컨텍스트 안에서 block_on 대신 futures::executor::block_on 사용 불가.
            // 대신 표준 스레드 채널(std::sync::mpsc)로 동기 호출 구현.
            let (tx, rx) = std::sync::mpsc::channel::<napi::Result<String>>();
            let reply_tx = Arc::new(Mutex::new(Some(tx)));

            tsfn.call(
                Ok((item, reply_tx)),
                ThreadsafeFunctionCallMode::Blocking,
            );

            match rx.recv() {
                Ok(Ok(json)) if json == "null" || json.is_empty() => None,
                Ok(Ok(json)) => serde_json::from_str::<WriteQuery>(&json).ok(),
                _ => None,
            }
        });

        // Processor 생성
        let processor = match processor_fn {
            Some(f) => ProcessorKind::JsBridge(JsProcessorDirect::new(f, 30_000)?),
            None    => ProcessorKind::None,
        };

        let (deferred, promise) = env.create_deferred::<JobExecution, _>()?;
        let options = self.options.clone();
        let stop_signal = self.stop_signal.clone();
        stop_signal.reset();

        tokio::spawn(async move {
            let result = run_native_chunk_loop(
                options, stop_signal, reader, processor, writer,
            ).await;

            match result {
                Ok(job_exec) => deferred.resolve(|_env| Ok(job_exec)),
                Err(e)       => deferred.reject(e),
            }
        });

        Ok(promise)
    }

    /// Chunk 지향 배치를 실행하고 JS Promise<JobExecution> 을 반환한다.
    ///
    /// 콜백 규약:
    ///   reader_fn    : (offset: number,    reply: (json: string) => void) => void
    ///   processor_fn : (itemsJson: string, reply: (json: string) => void) => void
    ///   writer_fn    : (itemsJson: string, reply: (json: string) => void) => void
    ///
    ///   reply 인자는 처리 완료 후 정확히 1회 호출. 오류 시 '__ERROR__:msg'.
    #[napi]
    pub fn execute(
        &self,
        env: Env,
        reader_fn: JsFunction,
        processor_fn: JsFunction,
        writer_fn: JsFunction,
    ) -> napi::Result<Object> {
        // ── ThreadsafeFunction 변환 (JS main thread 에서 해야 함) ──────────
        let build_tsfn = |f: JsFunction| -> napi::Result<CallTsfn> {
            f.create_threadsafe_function(
                0,
                |ctx: ThreadSafeCallContext<CallPayload>| {
                    let env = ctx.env;
                    let payload = ctx.value;

                    // arg0
                    let arg0: napi::JsUnknown = if payload.step == 0 {
                        env.create_uint32(payload.offset)?.into_unknown()
                    } else {
                        env.create_string(&payload.items_json)?.into_unknown()
                    };

                    // arg1: reply(json: string) — Rust 클로저를 NAPI 함수로 노출
                    let reply_tx_arc = Arc::clone(&payload.reply_tx);
                    let reply_fn: napi::JsFunction =
                        env.create_function_from_closure("reply", move |ctx| {
                            let json: String = ctx.get::<String>(0).unwrap_or_default();
                            // sender 를 소비하여 1회만 전송
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
            )
        };

        let reader_tsfn    = build_tsfn(reader_fn)?;
        let processor_tsfn = build_tsfn(processor_fn)?;
        let writer_tsfn    = build_tsfn(writer_fn)?;

        // ── JsDeferred: JS 에서 await 가능한 Promise 생성 ─────────────────
        let (deferred, promise) = env.create_deferred::<JobExecution, _>()?;

        let options     = self.options.clone();
        let stop_signal = self.stop_signal.clone();
        stop_signal.reset();

        // ── Tokio task spawn ──────────────────────────────────────────────
        // Tokio 런타임이 없으면 새로 생성 (napi tokio_rt feature 가 글로벌 런타임을 제공)
        tokio::spawn(async move {
            let result = run_chunk_loop(
                options,
                stop_signal,
                reader_tsfn,
                processor_tsfn,
                writer_tsfn,
            ).await;

            match result {
                Ok(job_exec)  => deferred.resolve(|_env| Ok(job_exec)),
                Err(e) => deferred.reject(e),
            }
        });

        Ok(promise)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Chunk 루프 (Tokio async context)
// ─────────────────────────────────────────────────────────────────────────────

/// JS 콜백을 NonBlocking 으로 호출하고 oneshot 채널로 결과를 await 한다.
async fn call_js(
    tsfn: &CallTsfn,
    payload: CallPayload,
    timeout: std::time::Duration,
) -> napi::Result<String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<napi::Result<String>>();

    // sender 를 payload.reply_tx 에 설정
    {
        let mut guard = payload.reply_tx.lock().unwrap();
        *guard = Some(tx);
    }

    // JS 이벤트 루프에 콜백 예약
    tsfn.call(Ok(payload), ThreadsafeFunctionCallMode::NonBlocking);

    // JS 완료 대기 (Tokio 비동기)
    match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(_))     => Err(napi::Error::from_reason("reply channel dropped")),
        Err(_)         => Err(napi::Error::from_reason(format!(
            "JS 콜백 타임아웃 ({}ms)", timeout.as_millis()
        ))),
    }
}

async fn run_chunk_loop(
    options: ChunkExecutorOptions,
    stop_signal: StopSignal,
    reader_tsfn: CallTsfn,
    processor_tsfn: CallTsfn,
    writer_tsfn: CallTsfn,
) -> napi::Result<JobExecution> {
    let skip_limit  = options.skip_limit.unwrap_or(0) as i64;
    let retry_limit = options.retry_limit.unwrap_or(3);
    let chunk_size  = options.chunk_size;
    let timeout     = std::time::Duration::from_secs(30);

    let mut job_exec  = JobExecution::new(&options.job_name);
    let mut step_exec = StepExecution::new(&options.step_name);

    info!(job=%options.job_name, step=%options.step_name, chunk_size, "Chunk 루프 시작");

    let mut offset: u32 = 0;

    loop {
        if stop_signal.is_stopped() {
            warn!(offset, "중단 신호 — 루프 종료");
            step_exec.complete(JobStatus::Stopped);
            job_exec.accumulate_step(&step_exec);
            job_exec.stop(Some("외부 중단 요청".into()));
            return Ok(job_exec);
        }

        // ── 1. Reader ─────────────────────────────────────────────────────
        debug!(offset, "Reader 호출");

        let reader_json = call_js(
            &reader_tsfn,
            CallPayload {
                step: 0, offset, items_json: String::new(),
                reply_tx: Arc::new(Mutex::new(None)),
            },
            timeout,
        ).await.map_err(|e| { error!(error=%e, "Reader 오류"); e })?;

        let items: Vec<Value> = match parse_reader_json(&reader_json)? {
            None    => { debug!("Reader null — 데이터 소진"); break; }
            Some(v) if v.is_empty() => { debug!("Reader 빈 배열"); break; }
            Some(v) => v,
        };
        let read_in_chunk = items.len() as i64;
        step_exec.read_count += read_in_chunk;
        debug!(read_in_chunk, "Reader 완료");

        // ── 2. Processor ──────────────────────────────────────────────────
        let items_json = serde_json::to_string(&items)
            .map_err(|e| napi::Error::from_reason(format!("직렬화 실패: {e}")))?;

        let proc_json = call_js(
            &processor_tsfn,
            CallPayload {
                step: 1, offset: 0, items_json,
                reply_tx: Arc::new(Mutex::new(None)),
            },
            timeout,
        ).await.map_err(|e| { error!(error=%e, "Processor 오류"); e })?;

        let processed_items = parse_array_json(&proc_json)?;
        let processed_count = processed_items.len() as i64;
        step_exec.process_count += processed_count;

        if skip_limit > 0 && step_exec.total_skip_count() > skip_limit {
            let msg = format!("skip_limit({skip_limit}) 초과");
            error!("{}", msg);
            step_exec.complete(JobStatus::Failed(msg.clone()));
            job_exec.accumulate_step(&step_exec);
            job_exec.fail(msg);
            return Ok(job_exec);
        }
        debug!(processed_count, "Processor 완료");

        // ── 3. Writer (재시도 포함) ────────────────────────────────────────
        let proc_json2 = serde_json::to_string(&processed_items)
            .map_err(|e| napi::Error::from_reason(format!("직렬화 실패: {e}")))?;

        match write_with_retry(&writer_tsfn, proc_json2, retry_limit, timeout).await {
            Ok(written_count) => {
                step_exec.write_count  += written_count;
                step_exec.commit_count += 1;
                debug!(written_count, commits=step_exec.commit_count, "Chunk 커밋");
            }
            Err(e) => {
                step_exec.rollback_count  += 1;
                step_exec.write_skip_count += processed_count;
                if skip_limit > 0 && step_exec.total_skip_count() <= skip_limit {
                    warn!(error=%e, "Writer 오류 — skip 내 계속");
                } else {
                    error!(error=%e, "Writer 오류 — Job FAILED");
                    step_exec.complete(JobStatus::Failed(e.to_string()));
                    job_exec.accumulate_step(&step_exec);
                    job_exec.fail(e.to_string());
                    return Ok(job_exec);
                }
            }
        }

        offset += chunk_size;
        if (read_in_chunk as u32) < chunk_size {
            debug!("마지막 페이지 — 루프 종료");
            break;
        }
    }

    step_exec.complete(JobStatus::Completed);
    job_exec.accumulate_step(&step_exec);
    job_exec.complete();

    info!(
        job=%job_exec.job_name, status=%job_exec.status,
        read=job_exec.read_count, written=job_exec.write_count,
        commits=job_exec.commit_count, "Job 완료"
    );
    Ok(job_exec)
}

async fn write_with_retry(
    writer_tsfn: &CallTsfn,
    items_json: String,
    retry_limit: u32,
    timeout: std::time::Duration,
) -> napi::Result<i64> {
    let count = parse_array_json(&items_json).map(|v| v.len() as i64).unwrap_or(0);
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match call_js(
            writer_tsfn,
            CallPayload {
                step: 2, offset: 0, items_json: items_json.clone(),
                reply_tx: Arc::new(Mutex::new(None)),
            },
            timeout,
        ).await {
            Ok(_) => return Ok(count),
            Err(e) if attempt <= retry_limit => {
                warn!(attempt, error=%e, "Writer 재시도");
                tokio::time::sleep(std::time::Duration::from_millis(100 * (1u64 << (attempt - 1)))).await;
            }
            Err(e) => {
                error!(attempt, error=%e, "Writer 최대 재시도 초과");
                return Err(e);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// JSON 헬퍼
// ─────────────────────────────────────────────────────────────────────────────

fn parse_reader_json(json: &str) -> napi::Result<Option<Vec<Value>>> {
    let s = json.trim();
    if s == "null" || s.is_empty() { return Ok(None); }
    match serde_json::from_str::<Value>(s)
        .map_err(|e| napi::Error::from_reason(format!("Reader JSON 파싱 실패: {e}")))?
    {
        Value::Null       => Ok(None),
        Value::Array(arr) => Ok(Some(arr)),
        other => Err(napi::Error::from_reason(format!(
            "Reader 는 배열 또는 null 을 반환해야 합니다: {other:?}"
        ))),
    }
}

fn parse_array_json(json: &str) -> napi::Result<Vec<Value>> {
    let s = json.trim();
    if s == "null" || s.is_empty() { return Ok(vec![]); }
    match serde_json::from_str::<Value>(s)
        .map_err(|e| napi::Error::from_reason(format!("배열 JSON 파싱 실패: {e}")))?
    {
        Value::Array(arr) => Ok(arr),
        Value::Null       => Ok(vec![]),
        other => Err(napi::Error::from_reason(format!("배열이 아닌 값: {other:?}"))),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Native Chunk 루프 (Rust DB I/O 직접 처리)
// ─────────────────────────────────────────────────────────────────────────────

/// Reader/Writer가 Rust sqlx로 직접 DB를 처리하는 청크 루프.
///
/// Processor 종류별 직렬화 횟수:
///   - ProcessorKind::None      → 0회 (Reader → Writer 직통)
///   - ProcessorKind::Native    → 0회 (rayon 병렬)
///   - ProcessorKind::JsBridge  → 2회 (JS 위임 시 불가피)
async fn run_native_chunk_loop(
    options: ChunkExecutorOptions,
    stop_signal: StopSignal,
    reader: DbReader,
    processor: ProcessorKind,
    writer: DbWriter,
) -> napi::Result<JobExecution> {
    let skip_limit  = options.skip_limit.unwrap_or(0) as i64;
    let retry_limit = options.retry_limit.unwrap_or(3);
    let chunk_size  = options.chunk_size;

    let mut job_exec  = JobExecution::new(&options.job_name);
    let mut step_exec = StepExecution::new(&options.step_name);

    info!(job=%options.job_name, step=%options.step_name, chunk_size, "Native Chunk 루프 시작");

    let mut offset: u32 = 0;

    loop {
        if stop_signal.is_stopped() {
            warn!(offset, "중단 신호 — Native 루프 종료");
            step_exec.complete(JobStatus::Stopped);
            job_exec.accumulate_step(&step_exec);
            job_exec.stop(Some("외부 중단 요청".into()));
            return Ok(job_exec);
        }

        // ── 1. Rust DbReader — sqlx SELECT (직렬화 없음) ─────────────────
        debug!(offset, "Native Reader 호출");
        let items = reader
            .read(offset, chunk_size)
            .await
            .map_err(|e| napi::Error::from_reason(format!("Native Reader 오류: {e}")))?;

        let Some(items) = items else {
            debug!("Native Reader null — 데이터 소진");
            break;
        };
        if items.is_empty() {
            break;
        }

        let read_in_chunk = items.len() as i64;
        step_exec.read_count += read_in_chunk;
        debug!(read_in_chunk, "Native Reader 완료");

        // ── 2. Processor (None/Native: 직렬화 0회, JsBridge: 2회) ────────
        let processed = processor
            .process(items)
            .await
            .map_err(|e| napi::Error::from_reason(format!("Processor 오류: {e}")))?;

        let processed_count = processed.len() as i64;
        step_exec.process_count += processed_count;

        if skip_limit > 0 && step_exec.total_skip_count() > skip_limit {
            let msg = format!("skip_limit({skip_limit}) 초과");
            error!("{}", msg);
            step_exec.complete(JobStatus::Failed(msg.clone()));
            job_exec.accumulate_step(&step_exec);
            job_exec.fail(msg);
            return Ok(job_exec);
        }

        // ── 3. Rust DbWriter — sqlx INSERT/UPDATE + 트랜잭션 (직렬화 없음) ─
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match writer.write(&processed).await {
                Ok(written) => {
                    step_exec.write_count  += written as i64;
                    step_exec.commit_count += 1;
                    debug!(written, commits=step_exec.commit_count, "Native Chunk 커밋");
                    break;
                }
                Err(e) if attempt <= retry_limit => {
                    step_exec.rollback_count += 1;
                    warn!(attempt, error=%e, "Native Writer 재시도");
                    tokio::time::sleep(
                        std::time::Duration::from_millis(100 * (1u64 << (attempt - 1)))
                    ).await;
                }
                Err(e) => {
                    step_exec.rollback_count  += 1;
                    step_exec.write_skip_count += processed_count;
                    if skip_limit > 0 && step_exec.total_skip_count() <= skip_limit {
                        warn!(error=%e, "Native Writer 오류 — skip 내 계속");
                        break;
                    } else {
                        error!(error=%e, "Native Writer 최대 재시도 초과 — Job FAILED");
                        step_exec.complete(JobStatus::Failed(e.to_string()));
                        job_exec.accumulate_step(&step_exec);
                        job_exec.fail(e.to_string());
                        return Ok(job_exec);
                    }
                }
            }
        }

        offset += chunk_size;
        if (read_in_chunk as u32) < chunk_size {
            debug!("Native 마지막 페이지 — 루프 종료");
            break;
        }
    }

    step_exec.complete(JobStatus::Completed);
    job_exec.accumulate_step(&step_exec);
    job_exec.complete();

    info!(
        job=%job_exec.job_name, status=%job_exec.status,
        read=job_exec.read_count, written=job_exec.write_count,
        commits=job_exec.commit_count, "Native Job 완료"
    );
    Ok(job_exec)
}

// ─────────────────────────────────────────────────────────────────────────────
// 단위 테스트
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn test_stop_signal() {
        let s = StopSignal::new(); assert!(!s.is_stopped());
        s.stop(); assert!(s.is_stopped());
        s.reset(); assert!(!s.is_stopped());
    }

    #[test] fn test_stop_signal_clone() {
        let s = StopSignal::new(); let s2 = s.clone();
        s.stop(); assert!(s2.is_stopped());
    }

    #[test] fn test_parse_reader_null()  { assert!(parse_reader_json("null").unwrap().is_none()); }
    #[test] fn test_parse_reader_array() { assert_eq!(parse_reader_json(r#"[1,2]"#).unwrap().unwrap().len(), 2); }
    #[test] fn test_parse_array()        { assert_eq!(parse_array_json(r#"[1,2,3]"#).unwrap().len(), 3); }
}
