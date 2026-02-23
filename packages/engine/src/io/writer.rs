use serde_json::Value;
use thiserror::Error;

use crate::io::connection::DbPool;

// ─────────────────────────────────────────────────────────────────────────────
// 에러 타입
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum WriterError {
    #[error("DB 쓰기 실패: {0}")]
    QueryFailed(#[from] sqlx::Error),

    #[error("WriteQuery 빌더가 None을 반환했습니다 (아이템 index={0})")]
    NullQuery(usize),
}

// ─────────────────────────────────────────────────────────────────────────────
// WriteQuery
//
// 아이템 하나를 받아 실행할 SQL과 바인딩 파라미터를 반환하는 구조체.
//
// JS에서 커링 함수로 전달받은 writer query를 표현한다.
//
//   // JS 쪽 예시
//   writer: {
//     query: (item) => ({
//       sql: 'INSERT INTO settled_users (id, final_points) VALUES ($1, $2)',
//       params: [item.id, item.finalPoints],
//     }),
//   }
// ─────────────────────────────────────────────────────────────────────────────

/// 아이템 하나에 대한 SQL 실행 단위.
#[derive(Debug, Clone)]
pub struct WriteQuery {
    /// 실행할 SQL 문
    pub sql: String,
    /// SQL 플레이스홀더에 바인딩할 값 배열
    pub params: Vec<Value>,
}

impl WriteQuery {
    pub fn new(sql: impl Into<String>, params: Vec<Value>) -> Self {
        Self {
            sql: sql.into(),
            params,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WriteQueryBuilder
//
// 각 아이템(Value)을 WriteQuery로 변환하는 함수 타입.
// JS에서 `(item) => ({ sql, params })` 커링 함수에 대응한다.
//
// Rust 내부에서는 `Arc<dyn Fn(&Value) -> Option<WriteQuery> + Send + Sync>`
// 형태로 보관한다.
// ─────────────────────────────────────────────────────────────────────────────

pub type WriteQueryBuilderFn = dyn Fn(&Value) -> Option<WriteQuery> + Send + Sync;

// ─────────────────────────────────────────────────────────────────────────────
// DbWriter
// ─────────────────────────────────────────────────────────────────────────────

/// Rust-native DB Writer.
///
/// chunk 내의 모든 아이템을 단일 트랜잭션으로 묶어 실행한다.
/// 하나라도 실패하면 chunk 전체를 롤백 — Spring Batch의 chunk 원자성을 보장한다.
///
/// NAPI 경계를 거치지 않으므로 JSON 직렬화 오버헤드가 없다.
pub struct DbWriter {
    pool: DbPool,
    /// 아이템 → WriteQuery 변환 함수 (JS 커링 함수에 대응)
    query_builder: std::sync::Arc<WriteQueryBuilderFn>,
}

impl DbWriter {
    pub fn new(
        pool: DbPool,
        query_builder: impl Fn(&Value) -> Option<WriteQuery> + Send + Sync + 'static,
    ) -> Self {
        Self {
            pool,
            query_builder: std::sync::Arc::new(query_builder),
        }
    }

    /// chunk 내 모든 아이템을 단일 트랜잭션으로 쓴다.
    ///
    /// - 모든 아이템 성공 → 커밋
    /// - 하나라도 실패 → 전체 롤백 후 Err 반환
    ///
    /// 반환값: 성공적으로 쓴 아이템 수
    pub async fn write(&self, items: &[Value]) -> Result<usize, WriterError> {
        match &self.pool {
            DbPool::Postgres(pool) => self.write_postgres(pool, items).await,
            DbPool::MySql(pool) => self.write_mysql(pool, items).await,
        }
    }

    // ── PostgreSQL (트랜잭션) ──────────────────────────────────────────────────

    async fn write_postgres(
        &self,
        pool: &sqlx::Pool<sqlx::Postgres>,
        items: &[Value],
    ) -> Result<usize, WriterError> {
        let mut tx = pool.begin().await?;
        let mut written = 0;

        for (idx, item) in items.iter().enumerate() {
            let wq = (self.query_builder)(item)
                .ok_or(WriterError::NullQuery(idx))?;

            let mut query = sqlx::query(&wq.sql);
            for param in &wq.params {
                query = bind_value_postgres(query, param);
            }

            query.execute(&mut *tx).await?;
            written += 1;
        }

        tx.commit().await?;
        Ok(written)
    }

    // ── MySQL (트랜잭션) ───────────────────────────────────────────────────────

    async fn write_mysql(
        &self,
        pool: &sqlx::Pool<sqlx::MySql>,
        items: &[Value],
    ) -> Result<usize, WriterError> {
        let mut tx = pool.begin().await?;
        let mut written = 0;

        for (idx, item) in items.iter().enumerate() {
            let wq = (self.query_builder)(item)
                .ok_or(WriterError::NullQuery(idx))?;

            let mut query = sqlx::query(&wq.sql);
            for param in &wq.params {
                query = bind_value_mysql(query, param);
            }

            query.execute(&mut *tx).await?;
            written += 1;
        }

        tx.commit().await?;
        Ok(written)
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
// 단위 테스트
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_write_query_builder() {
        // JS의 `(item) => ({ sql, params })` 에 대응하는 Rust 클로저
        let builder = |item: &Value| -> Option<WriteQuery> {
            Some(WriteQuery::new(
                "INSERT INTO settled_users (id, final_points) VALUES ($1, $2)",
                vec![item["id"].clone(), item["finalPoints"].clone()],
            ))
        };

        let item = json!({ "id": 1, "finalPoints": 4500 });
        let wq = builder(&item).unwrap();

        assert!(wq.sql.contains("INSERT INTO"));
        assert_eq!(wq.params[0], Value::from(1));
        assert_eq!(wq.params[1], Value::from(4500));
    }

    #[test]
    fn test_null_query_builder() {
        // None 반환 시 NullQuery 에러
        let builder = |_item: &Value| -> Option<WriteQuery> { None };
        let result = builder(&json!({}));
        assert!(result.is_none());
    }
}
