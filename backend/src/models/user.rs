use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;

/// A row from the users table.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,                       //primary key
    pub username: String,               //unique, used for login
    pub email: String,                  //email address.
    #[serde(skip_serializing)]
    pub password_hash: String,          //bcrypt hash. Never sent to the client.
    pub role: String,                   //"viewer" | "streamer" | "admin"
    pub created_at: DateTime<Utc>,      //signup time
}

/// Body for POST /api/auth/register.
#[derive(Debug, Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub password: String,
}

/// Body for POST /api/auth/login.
#[derive(Debug, Deserialize)]
pub struct LoginUser {
    pub username: String,
    pub password: String,
}

/// Inserts a new user and returns the full row.
/// Email is derived from the username for now. Unique-constraint violations
/// surface as 400 BadRequest with "username already taken" so the client can
/// show a friendly message instead of a generic DB error.
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

/// Look up a user by UUID.
pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<User>, AppError> {
    let user = sqlx::query_as::<_, User>(
        "SELECT id, username, email, password_hash, role, created_at FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(user)
}


/// Return every user, newest first. Admin-only.
pub async fn list_users(pool: &PgPool) -> Result<Vec<User>, AppError> {
    let users = sqlx::query_as::<_, User>(
        r#"
        SELECT id, username, email, password_hash, role, created_at
        FROM users
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(users)
}

/// Change a user's role. 404 if the id doesn't exist.
pub async fn update_role(
    pool: &PgPool,
    id: Uuid,
    role: &str,
) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"
        UPDATE users
        SET role = $1
        WHERE id = $2
        RETURNING id, username, email, password_hash, role, created_at
        "#,
    )
    .bind(role)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("user not found".to_string()))?;

    Ok(user)
}