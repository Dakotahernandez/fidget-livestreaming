use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Form, Router,
};
use serde::Deserialize;
use sqlx::PgPool;

use crate::config::Config;
use crate::errors::AppError;
use crate::models::stream;

/// Build the internal RTMP callback router.
///
/// These endpoints are called by nginx-rtmp itself when streamers
/// publish or disconnect — NOT by end users. They live under
/// `/api/internal/rtmp` so it's clear they're infrastructure plumbing.
pub fn router() -> Router<(PgPool, Config)> {
    Router::new()
        .route("/on_publish", post(on_publish))
        .route("/on_publish_done", post(on_publish_done))
}

/// Subset of the form fields nginx-rtmp sends with on_publish/on_publish_done.
///
/// nginx-rtmp sends a bunch of metadata (app, addr, flashver, tcurl, etc.),
/// but the only thing we actually care about is `name` — that's the stream
/// key the streamer published with.
#[derive(Debug, Deserialize)]
struct RtmpCallback {
    name: String,
}

/// `POST /api/internal/rtmp/on_publish`
///
/// Called by nginx-rtmp every time a client tries to publish an RTMP stream.
/// We look up the stream key in the database; if it exists we return 200
/// (nginx allows the publish) and flip `is_live` to true. If it doesn't
/// exist we return 403 (nginx kills the connection).
async fn on_publish(
    State((pool, _config)): State<(PgPool, Config)>,
    Form(payload): Form<RtmpCallback>,
) -> Result<Response, AppError> {
    let stream_key = payload.name;

    match stream::find_by_stream_key(&pool, &stream_key).await? {
        Some(s) => {
            stream::set_live(&pool, s.id).await?;
            tracing::info!(stream_key = %stream_key, "Accepted RTMP publish");
            Ok(StatusCode::OK.into_response())
        }
        None => {
            tracing::warn!(stream_key = %stream_key, "Rejected RTMP publish (unknown key)");
            Ok(StatusCode::FORBIDDEN.into_response())
        }
    }
}

/// `POST /api/internal/rtmp/on_publish_done`
///
/// Called by nginx-rtmp when a publishing client disconnects. We flip the
/// stream offline so it disappears from the home page. Always returns 200
/// even if the stream key is unknown — there's nothing useful to reject here,
/// and we don't want a flaky disconnect to look like a bug in nginx logs.
async fn on_publish_done(
    State((pool, _config)): State<(PgPool, Config)>,
    Form(payload): Form<RtmpCallback>,
) -> Result<Response, AppError> {
    let stream_key = payload.name;

    if let Some(s) = stream::find_by_stream_key(&pool, &stream_key).await? {
        stream::set_offline(&pool, s.id).await?;
        tracing::info!(stream_key = %stream_key, "Marked stream offline");
    }

    Ok(StatusCode::OK.into_response())
}
