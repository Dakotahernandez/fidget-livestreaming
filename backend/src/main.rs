mod config;
mod db;
mod errors;
mod models;
mod routes;
mod middleware;

use axum::{routing::get, Json, Router};
use serde_json::json;
use std::net::SocketAddr;
use tracing_subscriber;

async fn health_check() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new().route("/health", get(health_check));

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("Backend listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
