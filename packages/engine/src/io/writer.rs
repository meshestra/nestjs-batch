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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

            // prepared statement의 바이너리 프로토콜 타입 불일치를 피하기 위해
            // 파라미터를 SQL에 직접 보간한다.
            let sql = interpolate_params(&wq.sql, &wq.params);
            sqlx::query(&sql).execute(&mut *tx).await?;
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

            let sql = interpolate_params_mysql(&wq.sql, &wq.params);
            sqlx::query(&sql).execute(&mut *tx).await?;
            written += 1;
        }

        tx.commit().await?;
        Ok(written)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 헬퍼: $1/$2/... 플레이스홀더에 값을 직접 보간
//
// prepared statement의 바이너리 프로토콜 타입 불일치를 피하기 위해
// 파라미터를 SQL 문자열에 직접 삽입한다.
// SQL 인젝션 방지를 위해 각 타입별로 안전하게 이스케이프한다.
// ─────────────────────────────────────────────────────────────────────────────

/// PostgreSQL 용 ($1, $2, ...) 파라미터 보간
fn interpolate_params(sql: &str, params: &[Value]) -> String {
    let mut result = sql.to_string();
    // $N을 큰 인덱스부터 처리해야 $10이 $1로 잘못 치환되는 것을 방지한다.
    for (i, param) in params.iter().enumerate().rev() {
        let placeholder = format!("${}", i + 1);
        let literal = value_to_pg_literal(param);
        result = result.replace(&placeholder, &literal);
    }
    result
}

/// MySQL 용 (?) 파라미터 보간
fn interpolate_params_mysql(sql: &str, params: &[Value]) -> String {
    let mut result = String::new();
    let mut param_iter = params.iter();
    for ch in sql.chars() {
        if ch == '?' {
            if let Some(param) = param_iter.next() {
                result.push_str(&value_to_mysql_literal(param));
            } else {
                result.push('?');
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn value_to_pg_literal(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => if *b { "TRUE".to_string() } else { "FALSE".to_string() },
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(f) = n.as_f64() {
                // 소수점 표현 — 부동소수점 오차를 최소화하기 위해 충분한 정밀도 사용
                format!("{:.10}", f)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            } else {
                n.to_string()
            }
        }
        // 문자열은 작은따옴표로 감싸고 내부 작은따옴표를 이스케이프한다.
        Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        // 배열/오브젝트는 JSON 문자열로 변환
        other => format!("'{}'", other.to_string().replace('\'', "''")),
    }
}

fn value_to_mysql_literal(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => if *b { "1".to_string() } else { "0".to_string() },
        Value::Number(n) => n.to_string(),
        Value::String(s) => format!("'{}'", s.replace('\'', "\\'")),
        other => format!("'{}'", other.to_string().replace('\'', "\\'")),
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
