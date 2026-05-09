use axum::{
    extract::{Path, State},
    middleware as axum_mw,
    routing::{delete, get, post, put},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::errors::AppError;
use crate::middleware::auth::{require_auth, AuthUser};
use crate::models::stream::{self, CreateStream};
use crate::models:: user;

/// Build the streams sub-router.
///
/// Public routes: list live streams, get stream details, and join a stream.
/// Protected routes: create streams, list own streams, and force start/stop.
pub fn router() -> Router<(PgPool, Config)> {
    let public = Router::new()
        .route("/", get(list_streams))
        .route("/:id", get(get_stream))
        .route("/:id/join", post(join_stream));

    let protected = Router::new()
        .route("/", post(create))
        .route("/me", get(list_my_streams))
        .route("/:id", delete(delete_stream))
        .route("/:id/start", put(start))
        .route("/:id/stop", put(stop))
        .layer(axum_mw::from_fn(require_auth));

    public.merge(protected)
}

#[derive(Debug, Deserialize)]
struct JoinStreamRequest {
    password: Option<String>,
}

fn can_create_stream(role: &str) -> bool {
    role == "admin" || role == "streamer"
}

fn can_control_stream(role: &str, owns_stream: bool) -> bool {
    role == "admin" || (role == "streamer" && owns_stream)
}

/// `GET /api/streams/me` — list all streams for the authenticated user.
async fn list_my_streams(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<Value>>, AppError> {
    let streams = stream::list_user_streams(&pool, auth.user_id).await?;

    let payload: Vec<Value> = streams
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "user_id": s.user_id,
                "stream_key": s.stream_key,
                "title": s.title,
                "is_public": s.is_public,
                "requires_password": !s.is_public,
                "is_live": s.is_live,
                "started_at": s.started_at,
                "created_at": s.created_at,
            })
        })
        .collect();

    Ok(Json(payload))
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
                "is_public": s.is_public,
                "requires_password": !s.is_public,
                "is_live": s.is_live,
                "started_at": s.started_at,
            })
        })
        .collect();

    Ok(Json(payload))
}

/// `GET /api/streams/:id` — get stream details for watch page metadata.
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
        "title": s.title,
        "is_public": s.is_public,
        "requires_password": !s.is_public,
        "is_live": s.is_live,
        "started_at": s.started_at,
        "created_at": s.created_at,
    })))
}

/// `POST /api/streams/:id/join` — validate stream access and return playable HLS URL.
async fn join_stream(
    State((pool, config)): State<(PgPool, Config)>,
    Path(id): Path<Uuid>,
    Json(body): Json<JoinStreamRequest>,
) -> Result<Json<Value>, AppError> {
    let s = stream::find_by_id(&pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    if !s.is_public {
        let submitted = body.password.unwrap_or_default();
        let hash = s
            .access_password_hash
            .as_deref()
            .ok_or_else(|| AppError::Unauthorized("this stream is locked".to_string()))?;

        let valid = bcrypt::verify(&submitted, hash)
            .map_err(|_| AppError::Internal("failed to verify stream password".to_string()))?;

        if !valid {
            return Err(AppError::Unauthorized("invalid stream password".to_string()));
        }
    }

    let cloudfront_url = config.cloudfront_url.trim_end_matches('/');

    Ok(Json(serde_json::json!({
        "id": s.id,
        "title": s.title,
        "is_live": s.is_live,
        "stream_key": s.stream_key,
        "hls_url": format!("{}/{}/index.m3u8", cloudfront_url ,s.stream_key),
    })))
}

/// `POST /api/streams` — create a new stream (auth required).
async fn create(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Json(body): Json<CreateStream>,
) -> Result<Json<Value>, AppError> {

    let current_user = user::find_by_id(&pool, auth.user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;

    if !can_create_stream(&current_user.role) {
        return Err(AppError::Forbidden("only streamers can create streams".to_string()));
    }

    let title = body.title.as_deref().unwrap_or("Untitled Stream");
    let access_password = body.access_password.unwrap_or_default();

    let password_hash = if access_password.trim().is_empty() {
        None
    } else {
        Some(bcrypt::hash(&access_password, bcrypt::DEFAULT_COST)?)
    };

    let is_public = password_hash.is_none();

    let s = stream::create_stream(
        &pool,
        auth.user_id,
        title,
        is_public,
        password_hash.as_deref(),
    )
    .await?;

    Ok(Json(serde_json::json!({
        "id": s.id,
        "user_id": s.user_id,
        "stream_key": s.stream_key,
        "title": s.title,
        "is_public": s.is_public,
        "requires_password": !s.is_public,
        "is_live": s.is_live,
        "created_at": s.created_at,
    })))
}

/// `PUT /api/streams/:id/start` — mark stream as live (auth required).
async fn start(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let current_user = user::find_by_id(&pool, auth.user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;

    let existing_stream = stream::find_by_id(&pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    let owns_stream = existing_stream.user_id == auth.user_id;

    if !can_control_stream(&current_user.role, owns_stream) {
        println!("User is not allowed to start this stream");
        return Err(AppError::Forbidden(
            "you do not have permission to start this stream".to_string(),
        ));
    }

    let s = stream::set_live(&pool, id).await?;

    Ok(Json(serde_json::json!({
        "id": s.id,
        "is_live": s.is_live,
        "started_at": s.started_at,
    })))
}

/// `PUT /api/streams/:id/stop` — end a broadcast (auth required). The
/// stream record is deleted entirely so it cannot be restarted and won't
/// clutter the dashboard. To go live again the streamer creates a fresh
/// stream from the dashboard form.
async fn stop(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let current_user = user::find_by_id(&pool, auth.user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;

    let existing_stream = stream::find_by_id(&pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    let owns_stream = existing_stream.user_id == auth.user_id;

    if !can_control_stream(&current_user.role, owns_stream) {
        println!("User is not allowed to stop this stream");
        return Err(AppError::Forbidden(
            "you do not have permission to stop this stream".to_string(),
        ));
    }

    let _ = stream::set_offline(&pool, id).await?;

    Ok(Json(serde_json::json!({
        "id": id,
        "is_live": false,
        "deleted": true,
    })))
}

/// `DELETE /api/streams/:id` — remove an old stream record (auth required,
/// owner or admin). Used by the dashboard to prune past streams so the UI isn't
/// cluttered with non-active entries.
async fn delete_stream(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    let current_user = user::find_by_id(&pool, auth.user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;

    let existing_stream = stream::find_by_id(&pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    let owns_stream = existing_stream.user_id == auth.user_id;

    if !can_control_stream(&current_user.role, owns_stream) {
        println!("User is not allowed to delete this stream");

        return Err(AppError::Forbidden(
            "you do not have permission to delete this stream".to_string(),
        ));
    }

    let affected = stream::delete_stream(&pool, id).await?;

    if affected == 0 {
        return Err(AppError::NotFound("stream not found".to_string()));
    }

    Ok(Json(serde_json::json!({
        "id": id,
        "deleted": true,
    })))
}