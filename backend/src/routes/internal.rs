use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use serde::Deserialize;
use sqlx::PgPool;

use crate::config::Config;
use crate::errors::AppError;
use crate::models::{stream, user};

/// Build the internal RTMP callback router.
///
/// These endpoints are called by the media server (SRS) when streamers
/// publish or disconnect — NOT by end users. They live under
/// `/api/internal/rtmp` so it's clear they're infrastructure plumbing.
pub fn router() -> Router<(PgPool, Config)> {
    Router::new()
        .route("/on_publish", post(on_publish))
        .route("/on_publish_done", post(on_publish_done))
}

/// Stream key carried by either a SRS JSON callback or a legacy nginx-rtmp
/// form-encoded callback. SRS uses field `stream`; nginx-rtmp uses `name`.
#[derive(Debug, Deserialize)]
struct RtmpCallback {
    #[serde(default, alias = "name")]
    stream: String,
}

/// The backend's ffmpeg fans out to per-quality rendition publishes whose
/// stream names are `<key>_480p`, `<key>_360p`, etc. SRS calls on_publish
/// once per rendition; this strips the suffix so the DB lookup hits the
/// single base stream record.
///
/// Returns `(base_key, is_rendition)`. `is_rendition == true` means the
/// callback corresponds to a downscale variant rather than the source
/// publish — caller should accept it (so SRS is happy) but skip the
/// `is_live` state transitions, which only the source publish drives.
fn split_rendition_suffix(stream_key: &str) -> (&str, bool) {
    const SUFFIXES: &[&str] = &["_240p", "_360p", "_480p", "_720p", "_1080p"];
    for suffix in SUFFIXES {
        if let Some(base) = stream_key.strip_suffix(suffix) {
            return (base, true);
        }
    }
    (stream_key, false)
}

/// Parse either an `application/json` (SRS) or `application/x-www-form-urlencoded`
/// (nginx-rtmp) body. Keeping both supported avoids a flag day if anyone runs the
/// old media server image locally.
fn extract_callback(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<RtmpCallback, AppError> {
    let is_json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("application/json"))
        .unwrap_or(false);

    if is_json {
        serde_json::from_slice::<RtmpCallback>(body)
            .map_err(|e| AppError::BadRequest(format!("invalid JSON callback: {e}")))
    } else {
        serde_urlencoded::from_bytes::<RtmpCallback>(body)
            .map_err(|e| AppError::BadRequest(format!("invalid form callback: {e}")))
    }
}

/// `POST /api/internal/rtmp/on_publish`
///
/// Called by the media server every time a client tries to publish an RTMP
/// stream. We look up the stream key in the database; if it exists we return
/// 200 with body `0` (the magic value SRS treats as success — nginx-rtmp
/// ignores the body and just looks at the status) and flip `is_live` to true.
/// If it doesn't exist we return 403 so the publish is dropped.
async fn on_publish(
    State((pool, _config)): State<(PgPool, Config)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let payload = extract_callback(&headers, &body)?;
    let stream_key = payload.stream;
    let (base_key, is_rendition) = split_rendition_suffix(&stream_key);

    match stream::find_by_stream_key(&pool, base_key).await? {
        Some(s) => {

            let owner = user::find_by_id(&pool, s.user_id)
                .await?
                .ok_or_else(|| AppError::Unauthorized("stream owner not found".to_string()))?;

            if owner.role != "admin" && owner.role != "streamer" {
                println!("Blocked RTMP publish because stream owner is not allowed to stream");

                tracing::warn!(
                    stream_key = %stream_key,
                    owner_id = %s.user_id,
                    role = %owner.role,
                    "Rejected RTMP publish because owner role cannot stream"
                );

                return Ok(StatusCode::FORBIDDEN.into_response());
            }

            // Only the source publish drives is_live — rendition callbacks
            // arrive moments later and would just re-flip an already-live
            // stream, so skip them to keep DB churn down.
            if !is_rendition {
                stream::set_live(&pool, s.id).await?;
            }
            tracing::info!(
                stream_key = %stream_key,
                rendition = is_rendition,
                "Accepted RTMP publish"
            );
            Ok((StatusCode::OK, "0").into_response())
        }
        None => {
            tracing::warn!(stream_key = %stream_key, "Rejected RTMP publish (unknown key)");
            Ok(StatusCode::FORBIDDEN.into_response())
        }
    }
}

/// `POST /api/internal/rtmp/on_publish_done`
///
/// Called when a publishing client disconnects. We flip the stream offline
/// so it disappears from the home page. Always returns 200/`0` even if the
/// stream key is unknown — there's nothing useful to reject here, and we
/// don't want a flaky disconnect to look like a bug in media-server logs.
async fn on_publish_done(
    State((pool, _config)): State<(PgPool, Config)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let payload = extract_callback(&headers, &body)?;
    let stream_key = payload.stream;
    let (base_key, is_rendition) = split_rendition_suffix(&stream_key);

    // Rendition publishes can stop and restart independently of the source
    // (e.g. one libx264 encoder hiccups). Don't let those events flip the
    // user-visible `is_live` flag — only the source publish ending should.
    if is_rendition {
        return Ok((StatusCode::OK, "0").into_response());
    }

    if let Some(s) = stream::find_by_stream_key(&pool, base_key).await? {
        stream::set_offline(&pool, s.id).await?;
        tracing::info!(stream_key = %stream_key, "Marked stream offline");
    }

    Ok((StatusCode::OK, "0").into_response())
}
