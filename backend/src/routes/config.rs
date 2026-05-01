use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::env;

use crate::config::Config;

pub fn router() -> Router<(PgPool, Config)> {
    Router::new()
        .route("/cdn", get(get_cdn_config))
        .route("/rtc", get(get_rtc_config))
}

/// `GET /api/config/cdn` — returns the CloudFront domain for HLS playback
async fn get_cdn_config(
    State((_pool, config)): State<(PgPool, Config)>,
) -> Json<Value> {
    Json(json!({
        "cloudfront_domain": config.cloudfront_url,
    }))
}

/// `GET /api/config/rtc` — returns the ICE servers the WHEP viewer should
/// use. Driven by env vars so each deployment can plug in its own STUN /
/// TURN endpoints without rebuilding the frontend:
///   - `STUN_URL`        (default: `stun:stun.l.google.com:19302`)
///   - `TURN_URL`        (optional, e.g. `turn:turn.example.com:3478?transport=udp`)
///   - `TURN_USERNAME`   (optional, required if TURN_URL is set)
///   - `TURN_PASSWORD`   (optional, required if TURN_URL is set)
/// A public STUN default is included so first-time deployments still get
/// host-candidate-only WHEP working without any extra config.
async fn get_rtc_config() -> Json<Value> {
    let mut ice_servers: Vec<Value> = Vec::new();

    let stun_url = env::var("STUN_URL")
        .unwrap_or_else(|_| "stun:stun.l.google.com:19302".to_string());
    if !stun_url.is_empty() {
        ice_servers.push(json!({ "urls": stun_url }));
    }

    if let Ok(turn_url) = env::var("TURN_URL") {
        if !turn_url.is_empty() {
            let mut entry = json!({ "urls": turn_url });
            if let Ok(user) = env::var("TURN_USERNAME") {
                entry["username"] = json!(user);
            }
            if let Ok(pass) = env::var("TURN_PASSWORD") {
                entry["credential"] = json!(pass);
            }
            ice_servers.push(entry);
        }
    }

    Json(json!({ "ice_servers": ice_servers }))
}