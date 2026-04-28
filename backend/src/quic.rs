use sqlx::PgPool;
use std::collections::HashMap;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use uuid::Uuid;
use wtransport::{Endpoint, Identity, ServerConfig};

use crate::middleware::auth::Claims;
use crate::models::stream;

/// QUIC datagram fragment header size in bytes.
///
/// Layout: `[chunk_id: u32][frag_index: u16][frag_total: u16]`
const FRAGMENT_HEADER_SIZE: usize = 8;

/// If a chunk hasn't been fully rebuilt after receiving a newer chunk_id,
/// throw itout. This prevents memory buildup from partially-received chunks.
const MAX_PENDING_CHUNKS: usize = 3;

/// Start the QUIC/WebTransport ingest server and return the SHA-256 cert hash.
///
/// The server listens on `0.0.0.0:<port>` over UDP using a self-signed TLS
/// certificate. The returned hexencoded hash is served to browsers with
/// REST API so they can trust the certificate through the WebTransport
pub fn start_quic_server(
    port: u16,
    jwt_secret: String,
    rtmp_url: String,
    pool: PgPool,
) -> String {
    let identity =
        Identity::self_signed(["localhost", "127.0.0.1", "::1"]).expect("failed to create self-signed identity");

    // Use wtransport's built-in SHA-256 hash for the certificate.
    let leaf_cert = identity
        .certificate_chain()
        .as_slice()
        .first()
        .expect("empty certificate chain");
    let digest = leaf_cert.hash();
    let cert_hash_hex = hex::encode(digest.as_ref());

    tracing::info!(
        "QUIC ingest server starting on UDP port {port} (cert hash: {cert_hash_hex})"
    );

    let config = ServerConfig::builder()
        .with_bind_default(port)
        .with_identity(identity)
        .keep_alive_interval(Some(std::time::Duration::from_secs(3)))
        .build();

    tokio::spawn(async move {
        run_quic_server(config, jwt_secret, rtmp_url, pool).await;
    });

    cert_hash_hex
}

/// Main loop: accept WebTransport sessions and spawn a handler per session.
async fn run_quic_server(
    config: ServerConfig,
    jwt_secret: String,
    rtmp_url: String,
    pool: PgPool,
) {
    let server = match Endpoint::server(config) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to create QUIC endpoint: {e}");
            return;
        }
    };

    loop {
        let incoming_session = server.accept().await;

        let jwt_secret = jwt_secret.clone();
        let rtmp_url = rtmp_url.clone();
        let pool = pool.clone();

        tokio::spawn(async move {
            let session_request = match incoming_session.await {
                Ok(req) => req,
                Err(e) => {
                    tracing::warn!("WebTransport session error: {e}");
                    return;
                }
            };

            // The browser connects to a URL
            // example ->  https://host:port/ingest/<stream_id>?token=<jwt>
            let path = session_request.path().to_string();
            let (stream_id, token) = match parse_ingest_path(&path) {
                Some(v) => v,
                None => {
                    tracing::warn!("Invalid ingest path: {path}");
                    return;
                }
            };

            // Validate JWT.
            let user_id = match validate_token(&token, &jwt_secret) {
                Some(id) => id,
                None => {
                    tracing::warn!("Invalid token for stream {stream_id}");
                    return;
                }
            };

            // Verify stream ownership.
            let s = match stream::find_by_id(&pool, stream_id).await {
                Ok(Some(s)) if s.user_id == user_id => s,
                _ => {
                    tracing::warn!("Stream {stream_id} not found or not owned by user {user_id}");
                    return;
                }
            };

            // Accept the session.
            let connection = match session_request.accept().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to accept WebTransport session: {e}");
                    return;
                }
            };

            tracing::info!(
                "WebTransport session accepted for stream {stream_id} from {}",
                connection.remote_address()
            );

            handle_session(connection, pool, stream_id, s.stream_key, rtmp_url).await;
        });
    }
}

/// Parse `/ingest/<uuid>?token=<jwt>` from the WebTransport request path.
fn parse_ingest_path(path: &str) -> Option<(Uuid, String)> {
    // Path format: /ingest/<stream_id>?token=<jwt>
    let (path_part, query) = path.split_once('?').unwrap_or((path, ""));

    let stream_id_str = path_part.strip_prefix("/ingest/")?;
    let stream_id = Uuid::parse_str(stream_id_str).ok()?;

    let token = query
        .split('&')
        .find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            if key == "token" { Some(value.to_string()) } else { None }
        })?;

    Some((stream_id, token))
}

/// Validate a JWT and return the user_id if valid.
fn validate_token(token: &str, secret: &str) -> Option<Uuid> {
    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .ok()?;

    Some(token_data.claims.sub)
}

/// Handle a single WebTransport session: receive datagrams, reassemble
/// fragmented video chunks, and pipe complete chunks to ffmpeg.
async fn handle_session(
    connection: wtransport::Connection,
    pool: PgPool,
    stream_id: Uuid,
    stream_key: String,
    rtmp_url: String,
) {
    // Mark stream as live.
    if let Err(e) = stream::set_live(&pool, stream_id).await {
        tracing::error!("Failed to mark stream {stream_id} as live: {e}");
        return;
    }

    let destination = format!("{rtmp_url}/{stream_key}");
    tracing::info!("Starting ffmpeg ingest for stream {stream_id} → {destination}");

    // Spawn ffmpeg: read WebM from stdin → transcode to H.264/AAC → push RTMP.
    let ffmpeg_result = Command::new("ffmpeg")
        .args([
            "-fflags",
            "nobuffer",
            "-i",
            "pipe:0",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-tune",
            "zerolatency",
            "-g",
            "30",
            "-keyint_min",
            "30",
            "-sc_threshold",
            "0",
            "-force_key_frames",
            "expr:gte(t,n_forced*1)",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-f",
            "flv",
            &destination,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn();

    let mut child = match ffmpeg_result {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("Failed to spawn ffmpeg: {e}");
            let _ = stream::set_offline(&pool, stream_id).await;
            return;
        }
    };

    let mut stdin = match child.stdin.take() {
        Some(s) => s,
        None => {
            tracing::error!("Failed to open ffmpeg stdin");
            let _ = child.kill().await;
            let _ = stream::set_offline(&pool, stream_id).await;
            return;
        }
    };

    // Fragment reassembly state.
    // Key: chunk_id, Value: (total_fragments, received fragments indexed by position).
    let mut pending: HashMap<u32, (u16, HashMap<u16, Vec<u8>>)> = HashMap::new();

    // Receive QUIC datagrams until the connection closes.
    loop {
        match connection.receive_datagram().await {
            Ok(datagram) => {
                let data = datagram.as_ref();
                if data.len() < FRAGMENT_HEADER_SIZE {
                    continue;
                }

                let chunk_id = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
                let frag_index = u16::from_be_bytes([data[4], data[5]]);
                let frag_total = u16::from_be_bytes([data[6], data[7]]);
                let payload = &data[FRAGMENT_HEADER_SIZE..];

                let entry = pending
                    .entry(chunk_id)
                    .or_insert_with(|| (frag_total, HashMap::new()));

                entry.1.insert(frag_index, payload.to_vec());

                // Check if all fragments have arrived for this chunk.
                if entry.1.len() == frag_total as usize {
                    // Reassemble in order.
                    let mut assembled = Vec::new();
                    for i in 0..frag_total {
                        if let Some(frag) = entry.1.get(&i) {
                            assembled.extend_from_slice(frag);
                        }
                    }
                    pending.remove(&chunk_id);

                    // Write the reassembled chunk to ffmpeg.
                    if let Err(e) = stdin.write_all(&assembled).await {
                        tracing::error!("ffmpeg write error for stream {stream_id}: {e}");
                        break;
                    }
                }

                // delete stale incomplete chunks to prevent memory buildup.
                // Keep only the most recent MAX_PENDING_CHUNKS entries.
                while pending.len() > MAX_PENDING_CHUNKS {
                    if let Some(&oldest) = pending.keys().min() {
                        pending.remove(&oldest);
                    }
                }
            }
            Err(e) => {
                tracing::info!("WebTransport connection closed for stream {stream_id}: {e}");
                break;
            }
        }
    }

    // Clean up ffmpeg.
    drop(stdin);
    let _ = child.kill().await;
    let _ = child.wait().await;

    // Mark offline.
    if let Err(e) = stream::set_offline(&pool, stream_id).await {
        tracing::error!("Failed to mark stream {stream_id} offline: {e}");
    }

    tracing::info!("Ingest ended for stream {stream_id}");
}
