use axum::{Extension, Json};
use serde_json::json;
use std::sync::{Arc, RwLock};

/// Returns the QUIC server's self-signed cert hash so the browser can pass it
/// to `new WebTransport(url, { serverCertificateHashes: [...] })`.
pub async fn cert_hash(
    Extension(hash): Extension<Arc<RwLock<String>>>,
) -> Json<serde_json::Value> {
    let current = hash
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    Json(json!({
        "algorithm": "sha-256",
        "hash": current,
    }))
}
