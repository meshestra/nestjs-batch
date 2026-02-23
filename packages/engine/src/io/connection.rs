use sqlx::{postgres::PgPoolOptions, mysql::MySqlPoolOptions, Pool, Postgres, MySql};
use thiserror::Error;

// ─────────────────────────────────────────────────────────────────────────────
// 에러 타입
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("지원하지 않는 DB URL 스킴: {0} (postgres:// 또는 mysql:// 만 지원)")]
    UnsupportedScheme(String),

    #[error("DB 연결 실패: {0}")]
    ConnectFailed(#[from] sqlx::Error),
}

// ─────────────────────────────────────────────────────────────────────────────
// DB 드라이버 추상화
// ─────────────────────────────────────────────────────────────────────────────

/// Rust sqlx 커넥션 풀.
///
/// PostgreSQL / MySQL 을 단일 열거형으로 추상화한다.
/// `BatchModule.forRoot({ datasource: { url: "..." } })` 에서
/// URL 스킴을 보고 자동으로 드라이버를 선택한다.
#[derive(Clone, Debug)]
pub enum DbPool {
    Postgres(Pool<Postgres>),
    MySql(Pool<MySql>),
}

impl DbPool {
    /// DB URL로부터 커넥션 풀을 생성한다.
    ///
    /// - `postgres://user:pass@host/db` → PostgreSQL
    /// - `mysql://user:pass@host/db`    → MySQL / MariaDB
    pub async fn connect(url: &str, max_connections: u32) -> Result<Self, ConnectionError> {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            let pool = PgPoolOptions::new()
                .max_connections(max_connections)
                .connect(url)
                .await?;
            Ok(DbPool::Postgres(pool))
        } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
            let pool = MySqlPoolOptions::new()
                .max_connections(max_connections)
                .connect(url)
                .await?;
            Ok(DbPool::MySql(pool))
        } else {
            let scheme = url.split("://").next().unwrap_or(url).to_string();
            Err(ConnectionError::UnsupportedScheme(scheme))
        }
    }

    /// 풀이 살아있는지 확인한다 (헬스체크용).
    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        match self {
            DbPool::Postgres(pool) => {
                sqlx::query("SELECT 1").execute(pool).await?;
            }
            DbPool::MySql(pool) => {
                sqlx::query("SELECT 1").execute(pool).await?;
            }
        }
        Ok(())
    }

    /// 현재 활성 커넥션 수를 반환한다.
    pub fn size(&self) -> u32 {
        match self {
            DbPool::Postgres(pool) => pool.size(),
            DbPool::MySql(pool) => pool.size(),
        }
    }

    /// 최대 커넥션 수를 반환한다.
    pub fn max_connections(&self) -> u32 {
        match self {
            DbPool::Postgres(pool) => pool.options().get_max_connections(),
            DbPool::MySql(pool) => pool.options().get_max_connections(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 커넥션 옵션 (JS에서 NAPI로 전달받는 설정)
// ─────────────────────────────────────────────────────────────────────────────

/// JS 쪽 `BatchModule.forRoot({ datasource: { ... } })` 에서 전달받는 설정.
#[derive(Debug, Clone)]
pub struct DataSourceOptions {
    /// DB 연결 URL
    /// 예: "postgres://user:pass@localhost:5432/mydb"
    pub url: String,

    /// 최대 커넥션 풀 크기.
    /// Backpressure 역할을 한다 — 풀이 가득 차면 자동으로 대기.
    /// 기본값: 10
    pub max_connections: u32,
}

impl DataSourceOptions {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            max_connections: 10,
        }
    }

    pub fn with_max_connections(mut self, n: u32) -> Self {
        self.max_connections = n;
        self
    }
}
