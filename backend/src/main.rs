mod config;
mod db;
mod errors;
mod models;
mod routes;
mod middleware;
mod quic;
mod quic_chat;
mod transcode;

use axum::{routing::get, Extension, Json, Router};
use serde_json::json;
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};

use config::Config;

//Trivial liveness endpoint. Used by docker health checks and prod monitors.
async fn health_check() -> Json<serde_json::Value> {
    Json(json!({"status": "ok"}))
}

//Process entry point. Boots config, DB, the QUIC ingest server, and the HTTP API.
#[tokio::main]
async fn main() {
    //Load .env if it exists. Silent if missing - prod uses real env vars.
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt::init();

    let cfg = Config::from_env().expect("failed to load config");
    tracing::info!("Connecting to database…");

    let pool = db::init_pool(&cfg.database_url)
        .await
        .expect("failed to connect to database");

    db::ensure_admin_user(&pool, &cfg.admin_username, &cfg.admin_password)
        .await
        .expect("failed to ensure admin user from environment");

    tracing::info!("Database connected");

    //Loud startup warning if the WebRTC ICE candidate is missing or
    //loopback. SRS reads `$CANDIDATE` directly from its config, but
    //operators usually only check the backend logs — surfacing it here
    //makes the "WHIP/WHEP doesn't work in production" failure mode
    //immediately diagnosable.
    let candidate = std::env::var("CANDIDATE").unwrap_or_default();
    if candidate.is_empty() || candidate == "127.0.0.1" || candidate == "localhost" {
        tracing::warn!(
            candidate = %candidate,
            "CANDIDATE is unset or loopback. WebRTC (WHIP publish + WHEP \
             playback) will not work for remote clients. Set CANDIDATE to \
             the public host IP / DNS in .env and ensure UDP 8000 is open \
             in the security group."
        );
    } else {
        tracing::info!(candidate = %candidate, "WebRTC candidate configured");
    }

    //Shared in-memory chat state. Holds per-stream broadcast channels.
    let chat_state = quic_chat::ChatState::new();

    //Registry of running WHIP rendition-ladder ffmpeg supervisors.
    //Populated by `on_publish` (source key), drained by `on_publish_done`.
    let ladder_registry = transcode::new_registry();

    //Debouncer for `on_publish_done` so transient SRS renegotiations
    //(e.g. WHEP viewers churning the rtc_to_rtmp bridge) don't immediately
    //flip the stream offline.
    let offline_debouncer = routes::internal::new_debouncer();

    //Start the QUIC/WebTransport server. Returns the self-signed cert hash
    //which is served to browsers so they can trust it via serverCertificateHashes.
    let cert_hash = quic::start_quic_server(
        cfg.quic_port,
        cfg.jwt_secret.clone(),
        cfg.rtmp_url.clone(),
        pool.clone(),
        chat_state.clone(),
    );

    //Open CORS - fine because every state-changing route requires a JWT.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let state = (pool.clone(), cfg.clone());

    //Wire up the HTTP router. Each `nest` is a submodule from routes/.
    let app = Router::new()
        .route("/health", get(health_check))
        .nest("/api/auth", routes::auth::router())
        .nest("/api/admin", routes::admin::router())
        .nest("/api/chat", routes::chat::router())
        .nest("/api/role-applications", routes::role_applications::router())
        .nest("/api/streams", routes::streams::router())
        .nest("/api/internal/rtmp", routes::internal::router())
        .nest("/api/config", routes::config::router())
        .route("/api/transport/cert-hash", get(routes::ingest::cert_hash))
        .layer(Extension(cfg.jwt_secret.clone()))
        .layer(Extension(cert_hash))
        .layer(Extension(chat_state))
        .layer(Extension(ladder_registry))
        .layer(Extension(offline_debouncer))
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
