use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Maximum number of connections in the pool.
const MAX_CONNECTIONS: u32 = 10;

/// Create a connection pool from the given database URL.
pub async fn init_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect(database_url)
        .await
}
