//! # io 모듈
//!
//! Rust-native DB I/O 레이어.
//!
//! ## 역할
//!
//! 기존 구조에서 Reader/Writer는 JS(TypeScript)에 있었고,
//! Rust는 NAPI 콜백으로 JS를 호출하여 데이터를 JSON 문자열로 주고받았다.
//! 이로 인해 다음 문제가 발생했다:
//!
//! - chunk당 JSON 직렬화/역직렬화 6회 발생
//! - JS 단일 스레드 병목 (Rust 병렬성이 JS event loop 앞에서 막힘)
//! - Rust가 트랜잭션을 제어할 수 없음
//! - 에러 타입/스택 트레이스 손실
//! - cursor 기반 Reader 불가 (stateless offset 방식 강제)
//!
//! ## 해결
//!
//! `io` 모듈이 DB I/O를 Rust-native로 처리한다:
//!
//! ```text
//! 기존:  JS Reader → JSON → Rust → JSON → JS Writer
//! 변경:  Rust DbReader (sqlx) → [JS Processor] → Rust DbWriter (sqlx)
//! ```
//!
//! Processor가 없으면 JS 호출이 아예 없어 직렬화 0회.
//! Processor가 있으면 Reader 결과를 JSON으로 변환하여 JS에 전달하고
//! 처리 결과를 다시 받아 Writer에 전달한다 (직렬화 2회).
//!
//! ## 모듈 구조
//!
//! - [`connection`]: DB 커넥션 풀 (`DbPool`, PostgreSQL / MySQL)
//! - [`reader`]:    Rust-native SELECT 실행 (`DbReader`, `QueryDef`)
//! - [`writer`]:    Rust-native INSERT/UPDATE 실행 + 트랜잭션 (`DbWriter`, `WriteQuery`)

pub mod connection;
pub mod reader;
pub mod writer;

// 공개 재수출
pub use connection::{ConnectionError, DataSourceOptions, DbPool};
pub use reader::{DbReader, QueryDef, ReaderError};
pub use writer::{DbWriter, WriteQuery, WriteQueryBuilderFn, WriterError};
