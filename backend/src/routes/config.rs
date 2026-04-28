use axum::{
    extract::State,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::config::Config;

pub fn router() -> Router<(PgPool, Config)> {
    Router::new()
        .route("/cdn", get(get_cdn_config))
}

/// `GET /api/config/cdn` — returns the CloudFront domain for HLS playback
async fn get_cdn_config(
    State((_pool, config)): State<(PgPool, Config)>,
) -> Json<Value> {
    Json(json!({
        "cloudfront_domain": config.cloudfront_url,
    }))
}