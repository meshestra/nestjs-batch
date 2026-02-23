#![deny(clippy::all)]

//! # nestjs-batch-engine
//!
//! Rust 기반의 고성능 배치 실행 엔진.
//! NAPI-RS를 통해 Node.js에 바인딩되며, NestJS 배치 프레임워크의 핵심 실행부를 담당한다.
//!
//! ## 모듈 구조
//!
//! - [`chunk_executor`]: Chunk 지향 배치 실행 엔진 (`ChunkExecutor`, `ChunkExecutorOptions`)
//! - [`job_execution`]: 실행 상태 및 결과 타입 (`JobExecution`, `StepExecution`, `JobStatus`)
//!
//! ## 실행 흐름
//!
//! ```text
//! [NestJS JobLauncher]
//!       │
//!       ▼
//! [ChunkExecutor::execute(readerFn, processorFn, writerFn)]
//!       │
//!       ▼  (Tokio async loop)
//! ┌─────────────────────────────────────────────────────────┐
//! │  loop {                                                 │
//! │    items     = await readerFn(offset)   ← TS 콜백      │
//! │    processed = await processorFn(items) ← TS 콜백      │
//! │    await writerFn(processed)            ← TS 콜백      │
//! │    offset += chunk_size                                 │
//! │  }                                                      │
//! └─────────────────────────────────────────────────────────┘
//!       │
//!       ▼
//! [JobExecution] → NestJS JobRepository
//! ```

use napi_derive::napi;

// ─────────────────────────────────────────────────────────────────────────────
// 서브모듈
// ─────────────────────────────────────────────────────────────────────────────

pub mod chunk_executor;
pub mod compute;
pub mod io;
pub mod job_execution;

// ─────────────────────────────────────────────────────────────────────────────
// 공개 재수출 (NAPI 바인딩에서 사용할 타입들)
// ─────────────────────────────────────────────────────────────────────────────

pub use chunk_executor::{ChunkExecutor, ChunkExecutorOptions};
pub use job_execution::{JobExecution, JobStatus, StepExecution};

// ─────────────────────────────────────────────────────────────────────────────
// 엔진 버전 및 메타데이터
// ─────────────────────────────────────────────────────────────────────────────

/// 엔진 버전을 반환한다 (Cargo.toml의 version 필드와 동기화)
#[napi]
pub fn engine_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 엔진 빌드 정보를 반환한다
#[napi(object)]
pub struct EngineBuildInfo {
    /// 엔진 버전 (semver)
    pub version: String,
    /// Rust 컴파일러 버전
    pub rust_version: String,
    /// 빌드 타겟 트리플 (예: "aarch64-apple-darwin")
    pub target: String,
    /// 릴리즈 빌드 여부
    pub is_release: bool,
    /// NAPI-RS 버전
    pub napi_version: String,
}

/// 엔진의 빌드 정보를 반환한다.
///
/// 디버깅 및 호환성 확인 시 활용한다.
#[napi]
pub fn get_build_info() -> EngineBuildInfo {
    EngineBuildInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        rust_version: env!("CARGO_PKG_RUST_VERSION")
            .to_string()
            .is_empty()
            .then(|| "unknown".to_string())
            .unwrap_or_else(|| env!("CARGO_PKG_RUST_VERSION").to_string()),
        target: std::env::consts::ARCH.to_string(),
        is_release: !cfg!(debug_assertions),
        napi_version: "2".to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 헬스체크 / 연결 테스트
// ─────────────────────────────────────────────────────────────────────────────

/// Rust 엔진이 정상적으로 로드되었는지 확인하는 핑 함수.
///
/// NestJS 모듈 초기화 시 엔진 바인딩이 제대로 로드되었는지 검증하는 데 사용한다.
#[napi]
pub fn ping() -> String {
    "pong from nestjs-batch-engine (Rust)".to_string()
}

/// 비동기 핑 — Tokio 런타임이 정상 동작하는지 확인한다
#[napi]
pub async fn ping_async() -> napi::Result<String> {
    // 최소한의 비동기 작업으로 Tokio 런타임 동작 검증
    tokio::task::yield_now().await;
    Ok("async pong from nestjs-batch-engine (Rust + Tokio)".to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// 벤치마크 유틸리티
// ─────────────────────────────────────────────────────────────────────────────

/// Rust 측 고해상도 타임스탬프 (Unix epoch 기준, 밀리초)
///
/// JS의 `Date.now()`보다 정밀도가 높은 타임스탬프를 제공한다.
#[napi]
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 두 타임스탬프 사이의 경과 시간(ms)을 계산한다
#[napi]
pub fn elapsed_ms(start_ms: i64) -> i64 {
    now_ms() - start_ms
}

// ─────────────────────────────────────────────────────────────────────────────
// 내부 테스트
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_version_not_empty() {
        let v = engine_version();
        assert!(!v.is_empty(), "버전 문자열이 비어있으면 안 됩니다");
        // semver 형식 최소 검증 (x.y.z)
        assert!(v.contains('.'), "버전은 semver 형식이어야 합니다: {}", v);
    }

    #[test]
    fn test_ping() {
        let result = ping();
        assert!(result.contains("pong"), "ping은 'pong'을 포함해야 합니다");
        assert!(result.contains("Rust"), "ping은 'Rust'를 포함해야 합니다");
    }

    #[test]
    fn test_now_ms_reasonable() {
        let ts = now_ms();
        // 2024-01-01 00:00:00 UTC = 1_704_067_200_000 ms
        assert!(ts > 1_704_067_200_000, "타임스탬프가 너무 오래됐습니다: {}", ts);
    }

    #[test]
    fn test_elapsed_ms() {
        let start = now_ms();
        // 약간의 작업 후 경과 시간 확인
        let _: u64 = (0u64..10_000u64).sum();
        let elapsed = elapsed_ms(start);
        assert!(elapsed >= 0, "경과 시간은 음수일 수 없습니다: {}", elapsed);
    }

    #[test]
    fn test_get_build_info() {
        let info = get_build_info();
        assert!(!info.version.is_empty());
        assert!(!info.target.is_empty());
        assert_eq!(info.napi_version, "2");
    }
}
