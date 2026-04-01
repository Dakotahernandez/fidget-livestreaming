use axum::{
    extract::{Path, State},
    middleware as axum_mw,
    routing::{get, post, put},
    Extension, Json, Router,
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::errors::AppError;
use crate::middleware::auth::{require_auth, AuthUser};
use crate::models::stream::{self, CreateStream};

/// Build the streams sub-router.
///
/// Public routes (no auth): list live streams, get stream details.
/// Protected routes (auth required): create, start, stop.
pub fn router() -> Router<(PgPool, Config)> {
    let public = Router::new()
        .route("/", get(list_streams))
        .route("/:id", get(get_stream));

    let protected = Router::new()
        .route("/", post(create))
        .route("/:id/start", put(start))
        .route("/:id/stop", put(stop))
        .layer(axum_mw::from_fn(require_auth));

    public.merge(protected)
}

/// `GET /api/streams` — list all live streams.
async fn list_streams(
    State((pool, _config)): State<(PgPool, Config)>,
) -> Result<Json<Vec<Value>>, AppError> {
    let streams = stream::list_live_streams(&pool).await?;

    let payload: Vec<Value> = streams
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "user_id": s.user_id,
                "title": s.title,
                "is_live": s.is_live,
                "started_at": s.started_at,
            })
        })
        .collect();

    Ok(Json(payload))
}

/// `GET /api/streams/:id` — get stream details (including stream_key for the owner).
async fn get_stream(
    State((pool, _config)): State<(PgPool, Config)>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let s = stream::find_by_id(&pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    Ok(Json(serde_json::json!({
        "id": s.id,
        "user_id": s.user_id,
        "stream_key": s.stream_key,
        "title": s.title,
        "is_live": s.is_live,
        "started_at": s.started_at,
        "created_at": s.created_at,
    })))
}

/// `POST /api/streams` — create a new stream (auth required).
async fn create(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Json(body): Json<CreateStream>,
) -> Result<Json<Value>, AppError> {
    let title = body.title.as_deref().unwrap_or("Untitled Stream");
    let s = stream::create_stream(&pool, auth.user_id, title).await?;

    Ok(Json(serde_json::json!({
        "id": s.id,
        "user_id": s.user_id,
        "stream_key": s.stream_key,
        "title": s.title,
        "is_live": s.is_live,
        "created_at": s.created_at,
    })))
}

/// `PUT /api/streams/:id/start` — mark stream as live (auth required).
async fn start(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(_auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let s = stream::set_live(&pool, id).await?;

    Ok(Json(serde_json::json!({
        "id": s.id,
        "is_live": s.is_live,
        "started_at": s.started_at,
    })))
}

/// `PUT /api/streams/:id/stop` — mark stream as offline (auth required).
async fn stop(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(_auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let s = stream::set_offline(&pool, id).await?;

    Ok(Json(serde_json::json!({
        "id": s.id,
        "is_live": s.is_live,
    })))
}
