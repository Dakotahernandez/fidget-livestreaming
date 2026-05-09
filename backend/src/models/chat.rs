use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;

#[derive(Debug, Serialize, sqlx::FromRow, Clone)]
pub struct ChatMessage {
    pub id: Uuid,
    pub stream_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

pub async fn insert_message(
    pool: &PgPool,
    stream_id: Uuid,
    user_id: Uuid,
    message: &str,
) -> Result<ChatMessage, AppError> {
    let saved = sqlx::query_as::<_, ChatMessage>(
        r#"
        WITH inserted AS (
            INSERT INTO chat_messages (stream_id, user_id, message)
            VALUES ($1, $2, $3)
            RETURNING id, stream_id, user_id, message, created_at
        )
        SELECT
            inserted.id,
            inserted.stream_id,
            inserted.user_id,
            users.username,
            inserted.message,
            inserted.created_at
        FROM inserted
        JOIN users ON users.id = inserted.user_id
        "#,
    )
    .bind(stream_id)
    .bind(user_id)
    .bind(message)
    .fetch_one(pool)
    .await?;

    Ok(saved)
}

pub async fn list_recent_messages(
    pool: &PgPool,
    stream_id: Uuid,
    limit: i64,
) -> Result<Vec<ChatMessage>, AppError> {
    let messages = sqlx::query_as::<_, ChatMessage>(
        r#"
        SELECT
            chat_messages.id,
            chat_messages.stream_id,
            chat_messages.user_id,
            users.username,
            chat_messages.message,
            chat_messages.created_at
        FROM chat_messages
        JOIN users ON users.id = chat_messages.user_id
        WHERE chat_messages.stream_id = $1
        ORDER BY chat_messages.created_at DESC
        LIMIT $2
        "#,
    )
    .bind(stream_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    let mut messages = messages;
    messages.reverse();

    Ok(messages)
}