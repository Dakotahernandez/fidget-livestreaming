use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

/// Maximum number of connections in the pool.
const MAX_CONNECTIONS: u32 = 10;

/// Create a connection pool from the given database URL.
pub async fn init_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect(database_url)
        .await?;

    // Keep local developer databases forward-compatible when schema evolves.
    sqlx::query(
        r#"
        ALTER TABLE streams
        ADD COLUMN IF NOT EXISTS is_public BOOLEAN NOT NULL DEFAULT TRUE
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        ALTER TABLE streams
        ADD COLUMN IF NOT EXISTS access_password_hash VARCHAR(255)
        "#,
    )
    .execute(&pool)
    .await?;

        sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_messages (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            stream_id UUID NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
            user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            message TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            CHECK (char_length(message) > 0 AND char_length(message) <= 500)
        )
        "#,
    )
    .execute(&pool)
    .await?;
    
    Ok(pool)
}
