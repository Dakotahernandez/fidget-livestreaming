mod config;
mod db;
mod errors;
mod models;
mod routes;
mod middleware;
mod quic;

use axum::{routing::get, Extension, Json, Router};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use config::Config;

async fn health_check() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt::init();

    let cfg = Config::from_env().expect("failed to load config");
    tracing::info!("Connecting to database…");

    let pool = db::init_pool(&cfg.database_url)
        .await
        .expect("failed to connect to database");

    tracing::info!("Database connected");

    let cert_hash = quic::start_quic_server(
        cfg.quic_port,
        cfg.jwt_secret.clone(),
        cfg.rtmp_url.clone(),
        pool.clone(),
    );
    let cert_hash = Arc::new(cert_hash);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let state = (pool.clone(), cfg.clone());

    let app = Router::new()
        .route("/health", get(health_check))
        .nest("/api/auth", routes::auth::router())
        .nest("/api/streams", routes::streams::router())
        .nest("/api/internal/rtmp", routes::internal::router())
        .nest("/api/config", routes::config::router())
        .route("/api/transport/cert-hash", get(routes::ingest::cert_hash))
        .layer(Extension(cfg.jwt_secret.clone()))
        .layer(Extension(cert_hash))
        .layer(cors)
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("Backend listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind to port 8080");

    axum::serve(listener, app)
        .await
        .expect("server exited with error");
}
