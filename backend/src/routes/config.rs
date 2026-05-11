use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::env;

use crate::config::Config;

//Builds the /api/config/* sub-router.
pub fn router() -> Router<(PgPool, Config)> {
    Router::new()
        .route("/cdn", get(get_cdn_config))
        .route("/rtc", get(get_rtc_config))
}

/// GET /api/config/cdn - returns the CloudFront domain for HLS playback.
async fn get_cdn_config(
    State((_pool, config)): State<(PgPool, Config)>,
) -> Json<Value> {
    Json(json!({
        "cloudfront_domain": config.cloudfront_url,
    }))
}

/// GET /api/config/rtc - returns the ICE servers the WHEP viewer should use.
/// Driven by env vars so each deployment plugs in its own STUN/TURN without a
/// frontend rebuild: `STUN_URL`, `TURN_URL`, `TURN_USERNAME`, `TURN_PASSWORD`.
/// Defaults to public STUN so a fresh deploy gets host-candidate WHEP for free.
async fn get_rtc_config() -> Json<Value> {
    let mut ice_servers: Vec<Value> = Vec::new();

    //STUN: include even when empty isn't intended -public default keeps WHEP usable.
    let stun_url = env::var("STUN_URL")
        .unwrap_or_else(|_| "stun:stun.l.google.com:19302".to_string());
    if !stun_url.is_empty() {
        ice_servers.push(json!({ "urls": stun_url }));
    }

    //TURN: optional. Only emit if URL is set; attach creds if present.
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