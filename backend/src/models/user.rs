use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;

/// Stored user row.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

/// Payload for the registration endpoint.
#[derive(Debug, Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub password: String,
}

/// Payload for the login endpoint.
#[derive(Debug, Deserialize)]
pub struct LoginUser {
    pub username: String,
    pub password: String,
}

/// Insert a new user row. The email is derived from the username for now.
pub async fn create_user(
    pool: &PgPool,
    username: &str,
    password_hash: &str,
) -> Result<User, AppError> {
    let email = format!("{username}@fidget.local");

    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (username, email, password_hash)
        VALUES ($1, $2, $3)
        RETURNING id, username, email, password_hash, role, created_at
        "#,
    )
    .bind(username)
    .bind(&email)
    .bind(password_hash)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref db_err) if db_err.constraint().is_some() => {
            AppError::BadRequest("username already taken".to_string())
        }
        other => AppError::from(other),
    })?;

    Ok(user)
}

/// Look up a user by username.
pub async fn find_by_username(pool: &PgPool, username: &str) -> Result<Option<User>, AppError> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, username, email, password_hash, role, created_at FROM users WHERE username = $1",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;

    Ok(user)
}

/// Look up a user by their UUID.
pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<User>, AppError> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, username, email, password_hash, role, created_at FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(user)
}
