use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;

/// A row from the streams table.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Stream {
    pub id: Uuid,                                  //primary key
    pub user_id: Uuid,                             //owner
    pub stream_key: String,                        //RTMP ingest key. Secret.
    pub title: String,                             //display title on cards
    pub is_public: bool,                           //false = needs password to view
    #[serde(skip_serializing)]
    pub access_password_hash: Option<String>,      //bcrypt hash. Never sent out.
    pub is_live: bool,                             //flipped by SRS on_publish callbacks
    pub started_at: Option<DateTime<Utc>>,         //last go-live timestamp
    pub created_at: DateTime<Utc>,                 //row creation time
}

/// Body for POST /api/streams.
#[derive(Debug, Deserialize)]
pub struct CreateStream {
    pub title: Option<String>,
    pub access_password: Option<String>,
}

/// Insert a new stream row with a randomly generated key.
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

/// Return every stream currently live.
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

/// Return every stream owned by a user (including offline).
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

/// Look up a stream by its key.
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

/// Look up a stream by UUID.
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

/// Flip a stream to live and set started_at. 404 if not found.
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

/// Mark a stream offline by deleting its row. Returns rows affected.
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

/// Hard-delete a stream by id. Used by the dashboard prune button.
pub async fn delete_stream(pool: &PgPool, id: Uuid) -> Result<u64, AppError> {
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
