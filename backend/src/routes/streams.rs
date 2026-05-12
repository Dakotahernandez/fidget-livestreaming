use axum::{
    extract::{Path, State},
    http::HeaderMap,
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
use crate::middleware::auth::{
    create_chat_access_token,
    extract_optional_auth_user,
    require_auth,
    AuthUser,
    CHAT_TOKEN_EXPIRY_SECS,
};
use crate::models::stream::{self, CreateStream};
use crate::models::user;

/// Builds the /api/streams/* sub-router.
/// Public: list live, get details, join. Protected: create, list-mine, start,
/// stop, delete. Everything in `protected` runs through require_auth first.
pub fn router() -> Router<(PgPool, Config)> {
    let public = Router::new()
        .route("/", get(list_streams))
        .route("/:id", get(get_stream))
        .route("/:id/join", post(join_stream))

        // nginx routes
        .route("/on_publish", post(on_publish))
        .route("/on_done", post(on_done));

    let protected = Router::new()
        .route("/", post(create))
        .route("/me", get(list_my_streams))
        .route("/:id", delete(delete_stream))
        .route("/:id/start", put(start))
        .route("/:id/stop", put(stop))
        .layer(axum_mw::from_fn(require_auth));

    public.merge(protected)
}

//Body for POST /:id/join - password is optional (public streams ignore it).
#[derive(Debug, Deserialize)]
struct JoinStreamRequest {
    password: Option<String>,
}


#[derive(Debug, Deserialize)]
struct NginxCallback{
    name: String,
}

//Only admins and streamers can create streams.
fn can_create_stream(role: &str) -> bool {
    role == "admin" || role == "streamer"
}

//Admins can control anyone's stream. Streamers can only control their own.
fn can_control_stream(role: &str, owns_stream: bool) -> bool {
    role == "admin" || (role == "streamer" && owns_stream)
}

/// `GET /api/streams/me` - every stream the caller owns. Auth required.
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

/// `GET /api/streams` - every current-live stream. Powers the home page.
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

/// `GET /api/streams/:id` - stream details for the watch page. Public.
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

/// POST /api/streams/:id/join - verify access and return the HLS URL.
async fn join_stream(
    State((pool, config)): State<(PgPool, Config)>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<JoinStreamRequest>,
) -> Result<Json<Value>, AppError> {
    let s = stream::find_by_id(&pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    //Private stream: gate on the password hash.
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

    // When CLOUDFRONT_DOMAIN is set we hand viewers an absolute URL at
    // the configured CDN edge; otherwise we hand them a relative URL
    // that the frontend nginx proxies same-origin to the in-cluster
    // media-server (the local docker-compose dev path).
    let cloudfront_url = config.cloudfront_url.trim_end_matches('/');
    let hls_url = if cloudfront_url.is_empty() {
        format!("/hls/{}/index.m3u8", s.stream_key)
    } else {
        format!("{}/hls/{}/index.m3u8", cloudfront_url, s.stream_key)
    };

    let maybe_auth_user = extract_optional_auth_user(&headers, &config.jwt_secret)?;
    let chat_access_token = if let Some(auth_user) = maybe_auth_user {
        Some(create_chat_access_token(
            auth_user.user_id,
            s.id,
            &config.jwt_secret,
        )?)
    } else {
        None
    };

    Ok(Json(serde_json::json!({
        "id": s.id,
        "title": s.title,
        "is_live": s.is_live,
        "stream_key": s.stream_key,
        "hls_url": hls_url,
        "chat_access_token": chat_access_token,
        "chat_access_expires_in_secs": CHAT_TOKEN_EXPIRY_SECS,
    })))
}

/// `POST /api/streams` - create a stream. Auth required, streamer/admin only.
async fn create(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Json(body): Json<CreateStream>,
) -> Result<Json<Value>, AppError> {

    //Role check - viewers can't create streams.
    let current_user = user::find_by_id(&pool, auth.user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;

    if !can_create_stream(&current_user.role) {
        return Err(AppError::Forbidden("only streamers can create streams".to_string()));
    }

    let title = body.title.as_deref().unwrap_or("Untitled Stream");
    let access_password = body.access_password.unwrap_or_default();

    //Empty password -> public stream. Non-empty -> private with bcrypt hash.
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

/// `PUT /api/streams/:id/start` - flip a stream live. Owner or admin only.
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

/// `PUT /api/streams/:id/stop` - end a broadcast. Owner or admin only.
/// Hard-deletes the row. To go live again, the streamer creates a fresh
/// stream - each get a brand-new key.
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

/// `DELETE /api/streams/:id` - prune an old stream record. Owner or admin only.
/// Same effect as /stop but exposed as a separate endpoint so the dashboard's
/// "delete" button reads as deletion to the user, not stopping a broadcast.
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


async fn on_publish(
    State((pool, _config)): State<(PgPool, Config)>,
    axum::Form(payload): axum::Form<NginxCallback>,
) -> Result<axum::http::StatusCode, AppError> {
    let s = stream::find_by_stream_key(&pool, &payload.name)
        .await?
        .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    stream::set_live(&pool, s.id).await?;

    Ok(axum::http::StatusCode::OK)
}

async fn on_done(
    State((pool, _config)): State<(PgPool, Config)>,
    axum::Form(payload): axum::Form<NginxCallback>,
) -> Result<axum::http::StatusCode, AppError> {
    if let Some(s) = stream::find_by_stream_key(&pool, &payload.name).await? {
        stream::set_offline(&pool, s.id).await?;
    }
    Ok(axum::http::StatusCode::OK)
}