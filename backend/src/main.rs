mod config;
mod db;
mod errors;
mod models;
mod routes;
mod middleware;

use axum::{routing::get, Extension, Json, Router};
use serde_json::json;
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};

use config::Config;

async fn health_check() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

#[tokio::main]
async fn main() {
    // Load .env file if present (ignored in Docker where env is injected).
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt::init();

    let cfg = Config::from_env().expect("failed to load config");
    tracing::info!("Connecting to database…");

    let pool = db::init_pool(&cfg.database_url)
        .await
        .expect("failed to connect to database");

    tracing::info!("Database connected");

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Shared state: (PgPool, Config) — accessible via State extractor.
    let state = (pool.clone(), cfg.clone());

    let app = Router::new()
        .route("/health", get(health_check))
        .nest("/api/auth", routes::auth::router())
        .nest("/api/streams", routes::streams::router())
        // Inject the JWT secret as an extension so the auth middleware can read it.
        .layer(Extension(cfg.jwt_secret.clone()))
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
