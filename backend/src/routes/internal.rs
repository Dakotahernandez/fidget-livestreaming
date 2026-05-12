use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Extension, Router,
};
use serde::Deserialize;
use sqlx::PgPool;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::config::Config;
use crate::errors::AppError;
use crate::models::{stream, user};
use crate::transcode::{self, LadderRegistry};

/// How long to wait after `on_publish_done` before actually flipping the
/// stream offline. SRS occasionally fires `on_unpublish` for transient
/// internal renegotiations (a viewer joining/leaving via WHEP can churn
/// the `rtc_to_rtmp` bridge). If a fresh `on_publish` arrives during
/// this window, the pending offline is cancelled and the stream stays
/// live from the user's perspective.
const OFFLINE_DEBOUNCE: Duration = Duration::from_secs(3);

/// Tracks pending offline tasks per source stream id so a re-publish
/// inside the debounce window can cancel the offline transition.
pub type OfflineDebouncer = Arc<Mutex<HashMap<Uuid, JoinHandle<()>>>>;

/// Build a fresh, empty debouncer. Call once at startup and inject as an
/// Axum `Extension` alongside the ladder registry.
pub fn new_debouncer() -> OfflineDebouncer {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Cancel any pending offline transition for `stream_id`. Used by
/// `on_publish` to defeat a racing `on_publish_done` that fired moments
/// before the publisher reconnected.
async fn cancel_pending_offline(debouncer: &OfflineDebouncer, stream_id: Uuid) {
    if let Some(handle) = debouncer.lock().await.remove(&stream_id) {
        handle.abort();
    }
}

/// Builds the /api/internal/rtmp/* router.
pub fn router() -> Router<(PgPool, Config)> {
    Router::new()
        .route("/on_publish", post(on_publish))
        .route("/on_publish_done", post(on_publish_done))
}

/// Stream key from SRS (JSON `stream`) or legacy nginx-rtmp (form `name`).
#[derive(Debug, Deserialize)]
struct RtmpCallback {
    #[serde(default, alias = "name")]
    stream: String,
}

/// Strips the rendition suffix '_720p' off a stream key.
fn split_rendition_suffix(stream_key: &str) -> (&str, bool) {
    const SUFFIXES: &[&str] = &["_240p", "_360p", "_480p", "_720p", "_1080p"];
    for suffix in SUFFIXES {
        if let Some(base) = stream_key.strip_suffix(suffix) {
            return (base, true);
        }
    }
    (stream_key, false)
}

/// Parse a callback body. SRS sends JSON; nginx-rtmp sends form-encoded.
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

/// `POST /api/internal/rtmp/on_publish` - accept or reject an RTMP publish.
/// Accepts also flips the stream to live, but only for the source
async fn on_publish(
    State((pool, config)): State<(PgPool, Config)>,
    Extension(ladder): Extension<LadderRegistry>,
    Extension(debouncer): Extension<OfflineDebouncer>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let payload = extract_callback(&headers, &body)?;
    let stream_key = payload.stream;
    let (base_key, is_rendition) = split_rendition_suffix(&stream_key);

    match stream::find_by_stream_key(&pool, base_key).await? {
        Some(s) => {
            //Look up the owner - viewers can't publish, only admin/streamer roles can.
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

            //Source publish only
            if !is_rendition {
                //Defeat any pending offline transition from a recent
                //`on_publish_done` (transient SRS renegotiation, viewer
                //refresh churning the rtc_to_rtmp bridge, etc).
                cancel_pending_offline(&debouncer, s.id).await;
                stream::set_live(&pool, s.id).await?;
                //Spin up the rendition ladder. The WebTransport ingest path
                //already produces its own renditions inline; this branch
                //covers WHIP / direct-RTMP source publishes that arrive at
                //SRS without ever going through `quic.rs`.
                transcode::start_ladder(
                    &ladder,
                    s.id,
                    config.rtmp_url.clone(),
                    base_key.to_string(),
                )
                .await;
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

/// `POST /api/internal/rtmp/on_publish_done`  handle a publisher disconnect.
/// Schedules a debounced offline transition so a transient SRS
/// renegotiation (e.g. a viewer refreshing WHEP and churning the
/// `rtc_to_rtmp` bridge) doesn't immediately end the stream from the
/// user's perspective. If a fresh `on_publish` lands inside the
/// debounce window the pending task is cancelled and the stream stays
/// live.
async fn on_publish_done(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(ladder): Extension<LadderRegistry>,
    Extension(debouncer): Extension<OfflineDebouncer>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let payload = extract_callback(&headers, &body)?;
    let stream_key = payload.stream;
    let (base_key, is_rendition) = split_rendition_suffix(&stream_key);

    //Rendition stops/restarts independently of the source. Ignore them.
    if is_rendition {
        return Ok((StatusCode::OK, "0").into_response());
    }

    let Some(s) = stream::find_by_stream_key(&pool, base_key).await? else {
        return Ok((StatusCode::OK, "0").into_response());
    };

    //Replace any prior pending offline so the timer is always reset to
    //the most recent `on_publish_done`.
    cancel_pending_offline(&debouncer, s.id).await;

    let pool_clone = pool.clone();
    let ladder_clone = ladder.clone();
    let debouncer_clone = debouncer.clone();
    let stream_id = s.id;
    let stream_key_log = stream_key.clone();

    let handle = tokio::spawn(async move {
        tokio::time::sleep(OFFLINE_DEBOUNCE).await;
        match stream::set_offline(&pool_clone, stream_id).await {
            Ok(_) => {
                transcode::stop_ladder(&ladder_clone, stream_id).await;
                tracing::info!(
                    stream_key = %stream_key_log,
                    "Marked stream offline (debounced)"
                );
            }
            Err(e) => {
                tracing::warn!(
                    stream_key = %stream_key_log,
                    error = %e,
                    "Failed to mark stream offline after debounce"
                );
            }
        }
        debouncer_clone.lock().await.remove(&stream_id);
    });

    debouncer.lock().await.insert(stream_id, handle);
    tracing::info!(
        stream_key = %stream_key,
        "Scheduled debounced offline transition"
    );

    Ok((StatusCode::OK, "0").into_response())
}
