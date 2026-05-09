mod config;
mod db;
mod errors;
mod models;
mod routes;
mod middleware;
mod quic;

use axum::{extract::DefaultBodyLimit, middleware as axum_mw, routing::get, Extension, Json, Router};
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use config::Config;
use middleware::rate_limit::{rate_limit_layer, RateLimiter};
use routes::alerts::AlertSenders;

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

    // Per-stream SSE broadcaster. Subscribers register an mpsc sender
    // keyed by stream id; the Stripe webhook handler drains the map and
    // pushes the donation alert payload to every active viewer/streamer
    // connection. See routes/alerts.rs and routes/webhooks.rs.
    let alert_senders: AlertSenders = Arc::new(Mutex::new(HashMap::new()));

    // SECURITY: Per-route, per-IP rate limits. Limits are generous
    // enough that a real human never hits them but tight enough that a
    // scripted attacker can't burn Stripe API quota or fill the upload
    // disk. The donations limiter also caps how fast we can mint Stripe
    // Checkout sessions for a given source.
    let checkout_limiter = RateLimiter::new("donations.checkout", 10, 60);
    let upload_limiter = RateLimiter::new("sounds.upload", 20, 60);
    let onboard_limiter = RateLimiter::new("connect.onboard", 5, 60);
    let upload_body_cap = cfg.upload_max_bytes as usize + 1024; // small slack for multipart framing

    let state = (pool.clone(), cfg.clone());

    let app = Router::new()
        .route("/health", get(health_check))
        .nest("/api/auth", routes::auth::router())
        .nest("/api/streams", routes::streams::router())
        .nest("/api/internal/rtmp", routes::internal::router())
        .nest("/api/config", routes::config::router())
        .nest(
            "/api/connect",
            routes::connect::router().layer(axum_mw::from_fn({
                let lim = onboard_limiter.clone();
                move |req, next| rate_limit_layer(lim.clone(), req, next)
            })),
        )
        .nest(
            "/api/sounds",
            routes::sounds::router()
                // SECURITY: cap the multipart body at the configured
                // upload size + a small framing overhead so a hostile
                // client can't stream gigabytes through `field.bytes()`.
                .layer(DefaultBodyLimit::max(upload_body_cap))
                .layer(axum_mw::from_fn({
                    let lim = upload_limiter.clone();
                    move |req, next| rate_limit_layer(lim.clone(), req, next)
                })),
        )
        .nest(
            "/api/donations",
            routes::donations::router().layer(axum_mw::from_fn({
                let lim = checkout_limiter.clone();
                move |req, next| rate_limit_layer(lim.clone(), req, next)
            })),
        )
        .nest("/api/alerts", routes::alerts::router())
        .nest("/api/stripe", routes::webhooks::router())
        .route("/api/transport/cert-hash", get(routes::ingest::cert_hash))
        .nest_service("/uploads", ServeDir::new(&cfg.upload_dir))
        .layer(Extension(cfg.jwt_secret.clone()))
        .layer(Extension(cert_hash))
        .layer(Extension(alert_senders))
        .layer(cors)
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    tracing::info!("Backend listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind to port 8080");

    // SECURITY: `into_make_service_with_connect_info` is required for
    // the per-IP rate limiter to see the real source address via
    // `ConnectInfo<SocketAddr>`.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("server exited with error");
}
