use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;

/// Stored stream row.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Stream {
    pub id: Uuid,
    pub user_id: Uuid,
    pub stream_key: String,
    pub title: String,
    pub is_public: bool,
    #[serde(skip_serializing)]
    pub access_password_hash: Option<String>,
    pub is_live: bool,
    pub started_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Payload for creating a new stream.
#[derive(Debug, Deserialize)]
pub struct CreateStream {
    pub title: Option<String>,
    pub access_password: Option<String>,
}

/// Insert a new stream with a randomly generated key.
pub async fn create_stream(
    pool: &PgPool,
    user_id: Uuid,
    title: &str,
    is_public: bool,
    access_password_hash: Option<&str>,
) -> Result<Stream, AppError> {
    let stream_key = format!("live_{}", Uuid::new_v4().simple());

    let stream = sqlx::query_as::<_, Stream>(
        r#"
        INSERT INTO streams (user_id, stream_key, title, is_public, access_password_hash)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, user_id, stream_key, title, is_public, access_password_hash, is_live, started_at, created_at
        "#,
    )
    .bind(user_id)
    .bind(&stream_key)
    .bind(title)
    .bind(is_public)
    .bind(access_password_hash)
    .fetch_one(pool)
    .await?;

    Ok(stream)
}

/// Return all streams that are currently live.
pub async fn list_live_streams(pool: &PgPool) -> Result<Vec<Stream>, AppError> {
    let streams = sqlx::query_as::<_, Stream>(
        r#"
        SELECT id, user_id, stream_key, title, is_public, access_password_hash, is_live, started_at, created_at
        FROM streams
        WHERE is_live = true
        ORDER BY started_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(streams)
}

/// Return all streams owned by a user (live and offline).
pub async fn list_user_streams(pool: &PgPool, user_id: Uuid) -> Result<Vec<Stream>, AppError> {
    let streams = sqlx::query_as::<_, Stream>(
        r#"
        SELECT id, user_id, stream_key, title, is_public, access_password_hash, is_live, started_at, created_at
        FROM streams
        WHERE user_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(streams)
}

/// Look up a stream by its unique key (used for RTMP auth).
pub async fn find_by_stream_key(
    pool: &PgPool,
    stream_key: &str,
) -> Result<Option<Stream>, AppError> {
    let stream = sqlx::query_as::<_, Stream>(
        r#"
        SELECT id, user_id, stream_key, title, is_public, access_password_hash, is_live, started_at, created_at
        FROM streams
        WHERE stream_key = $1
        "#,
    )
    .bind(stream_key)
    .fetch_optional(pool)
    .await?;

    Ok(stream)
}

/// Look up a stream by id.
pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Stream>, AppError> {
    let stream = sqlx::query_as::<_, Stream>(
        r#"
        SELECT id, user_id, stream_key, title, is_public, access_password_hash, is_live, started_at, created_at
        FROM streams
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(stream)
}

/// Mark a stream as live.
pub async fn set_live(pool: &PgPool, id: Uuid) -> Result<Stream, AppError> {
    let stream = sqlx::query_as::<_, Stream>(
        r#"
        UPDATE streams
        SET is_live = true, started_at = NOW()
        WHERE id = $1
        RETURNING id, user_id, stream_key, title, is_public, access_password_hash, is_live, started_at, created_at
        "#,
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    Ok(stream)
}

/// Mark a stream offline by deleting the row. 
pub async fn set_offline(pool: &PgPool, id: Uuid) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        DELETE FROM streams
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Delete a stream by id, returning the number of affected rows. Streamers
/// can prune old stream records from the dashboard with this; the row is
/// gone for good (each new "Go Live" creates a fresh record via the create
/// endpoint).
pub async fn delete_stream(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        DELETE FROM streams
        WHERE id = $1 AND user_id = $2
        "#,
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}
