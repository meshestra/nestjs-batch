//! # compute 모듈
//!
//! Chunk 내 아이템 변환(Processor) 레이어.
//!
//! ## io vs compute
//!
//! ```text
//! io/      → 데이터 출입구 (DB READ / WRITE, I/O bound)
//! compute/ → 데이터 변환  (계산, CPU bound 또는 JS 위임)
//! ```
//!
//! ## 세 가지 처리 경로
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │  ProcessorKind::None                                    │
//! │    Reader → Writer 직통                                 │
//! │    직렬화 0회, JS 호출 0회                              │
//! │    단순 테이블 복사, ETL 변환 없는 케이스               │
//! └─────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────┐
//! │  ProcessorKind::Native (rayon)                          │
//! │    Reader → [rayon 병렬 변환] → Writer                  │
//! │    직렬화 0회, JS 호출 0회                              │
//! │    암호화, 세금 계산, 포맷 변환 등 CPU bound 케이스     │
//! └─────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────┐
//! │  ProcessorKind::JsBridge (Tokio oneshot)                │
//! │    Reader → JSON → JS Processor → JSON → Writer         │
//! │    직렬화 2회, JS 호출 1회/chunk                        │
//! │    TypeScript 비즈니스 로직이 필요한 케이스             │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 직렬화 비교
//!
//! | 경로        | 직렬화 횟수 | JS 호출 | 비고                  |
//! |-------------|-------------|---------|----------------------|
//! | None        | 0           | 0       | 최고 성능             |
//! | Native      | 0           | 0       | CPU 병렬, rayon       |
//! | JsBridge    | 2           | 1       | TS 로직 필요 시       |
//! | 기존 구조   | 6           | 3       | Reader+Processor+Writer 모두 JS |

pub mod js_bridge;
pub mod native;

pub use js_bridge::{JsBridgeError, JsProcessorDirect, ProcessorError, ProcessorKind};
pub use native::{NativeComputeError, NativeProcessor, TransformFn};
