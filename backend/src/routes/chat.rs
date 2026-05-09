use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::errors::AppError;
use crate::models::{chat, stream};

pub fn router() -> Router<(PgPool, Config)> {
    Router::new()
        .route("/streams/:id/messages", get(list_messages))
}

/// `GET /api/chat/streams/:id/messages` Loads recent chat messages for a stream.
async fn list_messages(
    State((pool, _config)): State<(PgPool, Config)>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<Value>>, AppError> {
    let _stream = stream::find_by_id(&pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    let messages = chat::list_recent_messages(&pool, id, 50).await?;

    let payload: Vec<Value> = messages
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "stream_id": m.stream_id,
                "user_id": m.user_id,
                "username": m.username,
                "message": m.message,
                "created_at": m.created_at,
            })
        })
        .collect();

    Ok(Json(payload))
}