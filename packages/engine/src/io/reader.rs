use serde_json::Value;
use sqlx::{Column, Row};
use thiserror::Error;

use crate::io::connection::DbPool;

// ─────────────────────────────────────────────────────────────────────────────
// 에러 타입
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ReaderError {
    #[error("DB 쿼리 실패: {0}")]
    QueryFailed(#[from] sqlx::Error),

    #[error("JSON 직렬화 실패: {0}")]
    SerializeFailed(#[from] serde_json::Error),

    #[error("지원하지 않는 DB 드라이버: {0}")]
    UnsupportedDriver(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// QueryBuilder
//
// JS에서 커링 함수로 전달받은 쿼리를 표현한다.
//
// 지원하는 형태:
//   1. 정적 문자열: "SELECT id, name FROM users"
//      → Rust가 LIMIT/OFFSET을 자동으로 붙인다.
//
//   2. 동적 빌더 (JS 커링 함수 결과): { sql: "...", params: [...] }
//      → JS가 직접 LIMIT/OFFSET을 포함한 완성된 쿼리를 반환.
// ─────────────────────────────────────────────────────────────────────────────

/// JS에서 전달받은 쿼리 표현.
#[derive(Debug, Clone)]
pub enum QueryDef {
    /// 정적 SQL. Rust가 `LIMIT $1 OFFSET $2` 를 자동으로 붙인다.
    Static(String),

    /// 동적 SQL. JS 커링 함수가 offset/limit 을 받아 완성된 쿼리를 반환.
    /// params는 SQL 플레이스홀더에 바인딩할 JSON 값 배열.
    Dynamic { sql: String, params: Vec<Value> },
}

impl QueryDef {
    /// 정적 쿼리에 LIMIT/OFFSET 절을 붙인다.
    ///
    /// PostgreSQL: `$1`, `$2` 플레이스홀더 사용
    /// MySQL:      `?`, `?` 플레이스홀더 사용
    pub fn with_pagination(&self, offset: u32, limit: u32, driver: &str) -> (String, Vec<Value>) {
        match self {
            QueryDef::Static(sql) => {
                let paginated = if driver == "mysql" {
                    format!("{sql} LIMIT ? OFFSET ?")
                } else {
                    format!("{sql} LIMIT $1 OFFSET $2")
                };
                let params = vec![Value::from(limit), Value::from(offset)];
                (paginated, params)
            }
            QueryDef::Dynamic { sql, params } => (sql.clone(), params.clone()),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// DbReader
// ─────────────────────────────────────────────────────────────────────────────

/// Rust-native DB Reader.
///
/// sqlx로 직접 SELECT를 실행하고 결과를 `Vec<Value>`로 반환한다.
/// NAPI 경계를 거치지 않으므로 JSON 직렬화 오버헤드가 없다.
///
/// Processor가 있는 경우에만 JSON으로 변환하여 JS로 전달한다.
#[derive(Clone)]
pub struct DbReader {
    pool: DbPool,
    query: QueryDef,
}

impl DbReader {
    pub fn new(pool: DbPool, query: QueryDef) -> Self {
        Self { pool, query }
    }

    /// `offset` 위치에서 `limit` 개의 row를 읽어 JSON Value 배열로 반환한다.
    ///
    /// 반환값:
    ///   - `Ok(Some(rows))` : 데이터 있음
    ///   - `Ok(None)`       : 데이터 소진 (offset >= 전체 행 수)
    ///   - `Err(_)`         : DB 오류
    pub async fn read(
        &self,
        offset: u32,
        limit: u32,
    ) -> Result<Option<Vec<Value>>, ReaderError> {
        let rows = match &self.pool {
            DbPool::Postgres(pool) => {
                let (sql, params) = self.query.with_pagination(offset, limit, "postgres");
                self.execute_postgres(pool, &sql, &params).await?
            }
            DbPool::MySql(pool) => {
                let (sql, params) = self.query.with_pagination(offset, limit, "mysql");
                self.execute_mysql(pool, &sql, &params).await?
            }
        };

        if rows.is_empty() {
            Ok(None) // 데이터 소진
        } else {
            Ok(Some(rows))
        }
    }

    // ── PostgreSQL ────────────────────────────────────────────────────────────

    async fn execute_postgres(
        &self,
        pool: &sqlx::Pool<sqlx::Postgres>,
        sql: &str,
        params: &[Value],
    ) -> Result<Vec<Value>, ReaderError> {
        // sqlx의 동적 바인딩: query() + bind() 체인
        // params는 serde_json Value → sqlx가 타입 추론하여 바인딩
        let mut query = sqlx::query(sql);
        for param in params {
            query = bind_value_postgres(query, param);
        }

        let rows = query
            .fetch_all(pool)
            .await?;

        // PgRow → serde_json::Value 변환
        rows.into_iter()
            .map(|row| pg_row_to_value(row))
            .collect::<Result<Vec<_>, _>>()
    }

    // ── MySQL ─────────────────────────────────────────────────────────────────

    async fn execute_mysql(
        &self,
        pool: &sqlx::Pool<sqlx::MySql>,
        sql: &str,
        params: &[Value],
    ) -> Result<Vec<Value>, ReaderError> {
        let mut query = sqlx::query(sql);
        for param in params {
            query = bind_value_mysql(query, param);
        }

        let rows = query
            .fetch_all(pool)
            .await?;

        rows.into_iter()
            .map(|row| mysql_row_to_value(row))
            .collect::<Result<Vec<_>, _>>()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 헬퍼: serde_json Value → sqlx 바인딩
// ─────────────────────────────────────────────────────────────────────────────

fn bind_value_postgres<'q>(
    query: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    value: &Value,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match value {
        Value::Null => query.bind(Option::<String>::None),
        Value::Bool(b) => query.bind(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                query.bind(i)
            } else {
                query.bind(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => query.bind(s.clone()),
        // 배열/객체는 JSON 문자열로 바인딩
        other => query.bind(other.to_string()),
    }
}

fn bind_value_mysql<'q>(
    query: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    value: &Value,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
    match value {
        Value::Null => query.bind(Option::<String>::None),
        Value::Bool(b) => query.bind(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                query.bind(i)
            } else {
                query.bind(n.as_f64().unwrap_or(0.0))
            }
        }
        Value::String(s) => query.bind(s.clone()),
        other => query.bind(other.to_string()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 헬퍼: sqlx Row → serde_json Value
// ─────────────────────────────────────────────────────────────────────────────

fn pg_row_to_value(row: sqlx::postgres::PgRow) -> Result<Value, ReaderError> {
    let columns = row.columns();
    let mut map = serde_json::Map::new();

    for col in columns {
        let name = col.name().to_string();
        // sqlx는 컬럼을 Value로 직접 decode할 수 없으므로
        // 타입별로 시도하여 첫 번째 성공한 값을 사용한다.
        let value = decode_pg_column(&row, col.ordinal());
        map.insert(name, value);
    }

    Ok(Value::Object(map))
}

fn decode_pg_column(row: &sqlx::postgres::PgRow, idx: usize) -> Value {

    // i64 시도
    if let Ok(v) = row.try_get::<i64, _>(idx) {
        return Value::from(v);
    }
    // f64 시도
    if let Ok(v) = row.try_get::<f64, _>(idx) {
        return Value::from(v);
    }
    // bool 시도
    if let Ok(v) = row.try_get::<bool, _>(idx) {
        return Value::from(v);
    }
    // String 시도 (TEXT, VARCHAR, UUID, TIMESTAMP 등)
    if let Ok(v) = row.try_get::<String, _>(idx) {
        return Value::String(v);
    }
    // serde_json::Value 시도 (JSON, JSONB 컬럼)
    if let Ok(v) = row.try_get::<Value, _>(idx) {
        return v;
    }
    // NULL 또는 미지원 타입
    Value::Null
}

fn mysql_row_to_value(row: sqlx::mysql::MySqlRow) -> Result<Value, ReaderError> {
    let columns = row.columns();
    let mut map = serde_json::Map::new();

    for col in columns {
        let name = col.name().to_string();
        let value = decode_mysql_column(&row, col.ordinal());
        map.insert(name, value);
    }

    Ok(Value::Object(map))
}

fn decode_mysql_column(row: &sqlx::mysql::MySqlRow, idx: usize) -> Value {

    if let Ok(v) = row.try_get::<i64, _>(idx) {
        return Value::from(v);
    }
    if let Ok(v) = row.try_get::<f64, _>(idx) {
        return Value::from(v);
    }
    if let Ok(v) = row.try_get::<bool, _>(idx) {
        return Value::from(v);
    }
    if let Ok(v) = row.try_get::<String, _>(idx) {
        return Value::String(v);
    }
    if let Ok(v) = row.try_get::<Value, _>(idx) {
        return v;
    }
    Value::Null
}

// ─────────────────────────────────────────────────────────────────────────────
// 단위 테스트
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_query_postgres_pagination() {
        let q = QueryDef::Static("SELECT id, name FROM users".into());
        let (sql, params) = q.with_pagination(40, 20, "postgres");
        assert!(sql.contains("LIMIT $1 OFFSET $2"));
        assert_eq!(params[0], Value::from(20u32)); // limit
        assert_eq!(params[1], Value::from(40u32)); // offset
    }

    #[test]
    fn test_static_query_mysql_pagination() {
        let q = QueryDef::Static("SELECT id, name FROM users".into());
        let (sql, params) = q.with_pagination(40, 20, "mysql");
        assert!(sql.contains("LIMIT ? OFFSET ?"));
        assert_eq!(params[0], Value::from(20u32));
        assert_eq!(params[1], Value::from(40u32));
    }

    #[test]
    fn test_dynamic_query_passthrough() {
        let q = QueryDef::Dynamic {
            sql: "SELECT * FROM users WHERE id > ? LIMIT ? OFFSET ?".into(),
            params: vec![Value::from(100), Value::from(20), Value::from(40)],
        };
        let (sql, params) = q.with_pagination(0, 0, "mysql"); // offset/limit 무시됨
        assert!(sql.contains("WHERE id >"));
        assert_eq!(params.len(), 3);
    }
}
