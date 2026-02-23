use rayon::prelude::*;
use serde_json::Value;
use thiserror::Error;

// ─────────────────────────────────────────────────────────────────────────────
// 에러 타입
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum NativeComputeError {
    #[error("변환 함수 실패 (index={index}): {reason}")]
    TransformFailed { index: usize, reason: String },

    #[error("JSON 직렬화 실패: {0}")]
    SerializeFailed(#[from] serde_json::Error),
}

// ─────────────────────────────────────────────────────────────────────────────
// TransformFn
//
// Rust-native Processor의 핵심 타입.
// JS의 `(item: T) => U` 에 대응하는 Rust 클로저.
//
// 조건:
//   - Send + Sync: rayon 스레드풀에서 안전하게 공유 가능해야 함
//   - 'static:     Tokio spawn_blocking에 넘길 수 있어야 함
// ─────────────────────────────────────────────────────────────────────────────

pub type TransformFn = dyn Fn(&Value) -> Result<Value, String> + Send + Sync + 'static;

// ─────────────────────────────────────────────────────────────────────────────
// NativeProcessor
//
// rayon으로 chunk 내 아이템을 멀티코어에서 병렬 변환한다.
//
// 사용 시나리오:
//   - 암호화 / 해시 계산
//   - 복잡한 세금·환율 계산
//   - 대량 데이터 포맷 변환
//
// 사용하지 않는 경우:
//   - 단순 필드 복사 (오버헤드가 이득보다 큼)
//   - DB 조회가 포함된 변환 (async 필요 → js_bridge 사용)
// ─────────────────────────────────────────────────────────────────────────────

pub struct NativeProcessor {
    transform: std::sync::Arc<TransformFn>,
    /// rayon 스레드풀. None이면 global pool 사용.
    pool: Option<rayon::ThreadPool>,
}

impl NativeProcessor {
    /// global rayon 스레드풀을 사용하는 Processor.
    /// 스레드 수 = CPU 코어 수 (rayon 기본값).
    pub fn new(transform: impl Fn(&Value) -> Result<Value, String> + Send + Sync + 'static) -> Self {
        Self {
            transform: std::sync::Arc::new(transform),
            pool: None,
        }
    }

    /// 전용 스레드풀을 사용하는 Processor.
    /// 다른 rayon 작업과 간섭하지 않아야 할 때 사용.
    pub fn with_thread_pool(
        transform: impl Fn(&Value) -> Result<Value, String> + Send + Sync + 'static,
        num_threads: usize,
    ) -> Result<Self, rayon::ThreadPoolBuildError> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()?;
        Ok(Self {
            transform: std::sync::Arc::new(transform),
            pool: Some(pool),
        })
    }

    /// chunk 내 아이템을 rayon으로 병렬 변환한다.
    ///
    /// - 각 아이템은 독립적으로 변환되므로 순서 보장됨 (par_iter 특성)
    /// - 하나라도 실패하면 Err 반환 (첫 번째 에러)
    pub fn process(&self, items: &[Value]) -> Result<Vec<Value>, NativeComputeError> {
        let transform = std::sync::Arc::clone(&self.transform);

        let run = || {
            items
                .par_iter()
                .enumerate()
                .map(|(idx, item)| {
                    transform(item).map_err(|reason| NativeComputeError::TransformFailed {
                        index: idx,
                        reason,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        };

        match &self.pool {
            Some(pool) => pool.install(run),
            None => run(),
        }
    }

    /// Tokio 컨텍스트에서 안전하게 호출하는 async 래퍼.
    ///
    /// rayon은 blocking 스레드풀이므로 Tokio에서 직접 호출하면
    /// async executor를 블로킹한다.
    /// `spawn_blocking`으로 감싸 Tokio worker thread를 보호한다.
    pub async fn process_async(
        &self,
        items: Vec<Value>,
    ) -> Result<Vec<Value>, NativeComputeError> {
        let transform = std::sync::Arc::clone(&self.transform);

        // spawn_blocking: rayon blocking 작업을 별도 OS 스레드에서 실행
        tokio::task::spawn_blocking(move || {
            items
                .par_iter()
                .enumerate()
                .map(|(idx, item)| {
                    transform(item).map_err(|reason| NativeComputeError::TransformFailed {
                        index: idx,
                        reason,
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .await
        // JoinError (패닉 등) → NativeComputeError로 변환
        .map_err(|e| NativeComputeError::TransformFailed {
            index: 0,
            reason: e.to_string(),
        })?
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 단위 테스트
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_users(n: usize) -> Vec<Value> {
        (0..n)
            .map(|i| json!({ "id": i, "raw_points": 10000 }))
            .collect()
    }

    #[test]
    fn test_parallel_transform() {
        // 포인트 10% 공제 변환
        let processor = NativeProcessor::new(|item| {
            let raw = item["raw_points"].as_i64().ok_or("raw_points 없음")?;
            Ok(json!({
                "id": item["id"],
                "final_points": (raw as f64 * 0.9) as i64,
            }))
        });

        let items = make_users(100);
        let result = processor.process(&items).unwrap();

        assert_eq!(result.len(), 100);
        assert_eq!(result[0]["final_points"], json!(9000i64));
    }

    #[test]
    fn test_transform_error_propagation() {
        let processor = NativeProcessor::new(|item| {
            if item["id"].as_i64().unwrap_or(0) == 5 {
                return Err("id=5는 처리 불가".into());
            }
            Ok(item.clone())
        });

        let items = make_users(10);
        let result = processor.process(&items);

        assert!(result.is_err());
        if let Err(NativeComputeError::TransformFailed { index, reason }) = result {
            assert_eq!(index, 5);
            assert!(reason.contains("id=5"));
        }
    }

    #[test]
    fn test_custom_thread_pool() {
        // 2 스레드 전용 풀
        let processor = NativeProcessor::with_thread_pool(
            |item| Ok(item.clone()),
            2,
        ).unwrap();

        let items = make_users(20);
        let result = processor.process(&items).unwrap();
        assert_eq!(result.len(), 20);
    }

    #[tokio::test]
    async fn test_process_async() {
        let processor = NativeProcessor::new(|item| {
            let raw = item["raw_points"].as_i64().ok_or("없음")?;
            Ok(json!({ "id": item["id"], "final_points": raw * 9 / 10 }))
        });

        let items = make_users(50);
        let result = processor.process_async(items).await.unwrap();
        assert_eq!(result.len(), 50);
        assert_eq!(result[0]["final_points"], json!(9000i64));
    }
}
