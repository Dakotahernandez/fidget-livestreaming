use axum::{Extension, Json};
use serde_json::json;
use std::sync::Arc;

pub async fn cert_hash(
    Extension(hash): Extension<Arc<String>>,
) -> Json<serde_json::Value> {
    Json(json!({
        "algorithm": "sha-256",
        "hash": hash.as_str(),
    }))
}
