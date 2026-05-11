use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

//Max connections in our pool.
const MAX_CONNECTIONS: u32 = 10;

/// Builds the Postgres pool and runs idempotent schema patches so older dev
/// databases don't need a manual migration step.
pub async fn init_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect(database_url)
        .await?;

    //Add is_public column if the dev DB predates it.
    sqlx::query(
        r#"
        ALTER TABLE streams
        ADD COLUMN IF NOT EXISTS is_public BOOLEAN NOT NULL DEFAULT TRUE
        "#,
    )
    .execute(&pool)
    .await?;

    //Add a private-stream password hash column if the dev DB predates it.
    sqlx::query(
        r#"
        ALTER TABLE streams
        ADD COLUMN IF NOT EXISTS access_password_hash VARCHAR(255)
        "#,
    )
    .execute(&pool)
    .await?;

    //Create chat_messages if missing.
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
