use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use crate::errors::AppError;

//Max connections in our pool.
const MAX_CONNECTIONS: u32 = 10;

/// Role name used for admin accounts.
const ADMIN_ROLE: &str = "admin";

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

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS role_applications (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            target_role VARCHAR(20) NOT NULL DEFAULT 'streamer',
            reason TEXT,
            status VARCHAR(20) NOT NULL DEFAULT 'pending',
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            reviewed_at TIMESTAMPTZ,
            reviewed_by UUID REFERENCES users(id) ON DELETE SET NULL,
            CHECK (target_role IN ('streamer', 'admin')),
            CHECK (status IN ('pending', 'approved', 'denied'))
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_role_applications_one_pending_per_user
        ON role_applications(user_id)
        WHERE status = 'pending'
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

/// Ensure there is an admin account matching credentials from the environment.
///
/// If the username already exists, this updates its password hash and role.
pub async fn ensure_admin_user(
    pool: &PgPool,
    admin_username: &str,
    admin_password: &str,
) -> Result<(), AppError> {
    if admin_username.trim().is_empty() || admin_password.is_empty() {
        return Err(AppError::BadRequest(
            "ADMIN_USERNAME and ADMIN_PASSWORD must be non-empty".to_string(),
        ));
    }

    let email = format!("{}@fidget.local", admin_username.trim());
    let password_hash = bcrypt::hash(admin_password, bcrypt::DEFAULT_COST)?;

    sqlx::query(
        r#"
        INSERT INTO users (username, email, password_hash, role)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (username)
        DO UPDATE SET
            password_hash = EXCLUDED.password_hash,
            role = EXCLUDED.role
        "#,
    )
    .bind(admin_username.trim())
    .bind(email)
    .bind(password_hash)
    .bind(ADMIN_ROLE)
    .execute(pool)
    .await?;

    Ok(())
}
