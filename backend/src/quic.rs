use sqlx::PgPool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;
use wtransport::{Endpoint, Identity, ServerConfig};

use crate::middleware::auth::Claims;
use crate::models::stream;

/// Read buffer size for the WebTransport unidirectional stream that carries
/// the WebM byte stream from the browser. Sized to comfortably hold a
/// MediaRecorder timeslice (~100ms of VP8/Opus) without excessive syscalls.
const STREAM_READ_BUF: usize = 64 * 1024;

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
    tracing::info!("Starting WebTransport ingest for stream {stream_id} → {destination}");

    // Fragment reassembly state removed: we now use a single ordered
    // unidirectional WebTransport stream that delivers the WebM byte stream
    // in order. This eliminates the cross-browser flakiness of QUIC
    // datagrams (Firefox in particular silently drops them on many paths).
    let mut total_bytes_written: u64 = 0;
    let mut reads: u64 = 0;

    tracing::info!(stream_id = %stream_id, "Waiting for unidirectional WebTransport stream");

    let mut recv_stream = match connection.accept_uni().await {
        Ok(s) => {
            tracing::info!(stream_id = %stream_id, "Unidirectional stream accepted, entering read loop");
            s
        }
        Err(e) => {
            tracing::warn!(stream_id = %stream_id, "Failed to accept unidirectional stream: {e}");
            let _ = stream::set_offline(&pool, stream_id).await;
            return;
        }
    };

    // Detect the container format the browser is sending before we spawn
    // ffmpeg, so we can `-c:v copy` H.264/MP4 inputs and skip the
    // libx264 transcode (which adds 200-400 ms of latency).
    let mut buf = vec![0u8; STREAM_READ_BUF];
    let mut prefix: Vec<u8> = Vec::with_capacity(8 * 1024);
    let detected = loop {
        match recv_stream.read(&mut buf).await {
            Ok(Some(0)) | Ok(None) => {
                tracing::warn!(stream_id = %stream_id, "stream ended before container header arrived");
                let _ = stream::set_offline(&pool, stream_id).await;
                return;
            }
            Ok(Some(n)) => {
                prefix.extend_from_slice(&buf[..n]);
                if let Some((fmt, offset)) = detect_input_format(&prefix) {
                    tracing::info!(
                        stream_id = %stream_id,
                        format = ?fmt,
                        skipped = offset,
                        prefix_len = prefix.len(),
                        "container detected"
                    );
                    break (fmt, offset);
                }
                if prefix.len() > 64 * 1024 {
                    tracing::error!(
                        stream_id = %stream_id,
                        "no recognised container header in first 64KB; aborting ingest"
                    );
                    let _ = stream::set_offline(&pool, stream_id).await;
                    return;
                }
            }
            Err(e) => {
                tracing::info!("WebTransport stream read error for {stream_id}: {e}");
                let _ = stream::set_offline(&pool, stream_id).await;
                return;
            }
        }
    };
    let (input_format, header_offset) = detected;

    let ffmpeg_result = spawn_ffmpeg(input_format, &destination);
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

    // Drain ffmpeg's stderr so its pipe buffer never fills up and stalls encoding.
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::info!(stream_id = %stream_id, "ffmpeg: {line}");
            }
        });
    }

    // Flush the buffered prefix (minus any leading priming bytes) to ffmpeg.
    if header_offset < prefix.len() {
        let initial = &prefix[header_offset..];
        if let Err(e) = stdin.write_all(initial).await {
            tracing::error!("ffmpeg write error for stream {stream_id}: {e}");
            let _ = child.kill().await;
            let _ = stream::set_offline(&pool, stream_id).await;
            return;
        }
        total_bytes_written += initial.len() as u64;
    }
    drop(prefix);

    loop {
        match recv_stream.read(&mut buf).await {
            Ok(Some(0)) | Ok(None) => {
                tracing::info!(stream_id = %stream_id, "WebTransport stream ended (EOF)");
                break;
            }
            Ok(Some(n)) => {
                reads += 1;
                total_bytes_written += n as u64;
                if reads == 1 || reads.is_multiple_of(50) {
                    tracing::info!(
                        stream_id = %stream_id,
                        reads,
                        bytes = total_bytes_written,
                        "Read bytes from WT stream"
                    );
                }

                if let Err(e) = stdin.write_all(&buf[..n]).await {
                    tracing::error!("ffmpeg write error for stream {stream_id}: {e}");
                    break;
                }
            }
            Err(e) => {
                tracing::info!("WebTransport stream read error for {stream_id}: {e}");
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

/// Container format the browser is sending over WebTransport. The choice
/// drives whether ffmpeg has to transcode (`WebM` → libx264) or can copy
/// the H.264 elementary stream straight into RTMP (`Mp4`).
#[derive(Debug, Clone, Copy)]
enum InputFormat {
    /// VP8 + Opus inside a Matroska/WebM container (Firefox + older Chrome).
    WebM,
    /// H.264 + AAC inside fragmented MP4 (Chrome >= 130).
    Mp4,
}

/// Inspect the first bytes of the WebTransport stream and identify the
/// container format. Returns the format together with the byte offset
/// where the real container header starts so the caller can strip any
/// priming bytes the browser sent before the first cluster/box.
fn detect_input_format(data: &[u8]) -> Option<(InputFormat, usize)> {
    const EBML_MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

    if let Some(idx) = data.windows(4).position(|w| w == EBML_MAGIC) {
        return Some((InputFormat::WebM, idx));
    }
    if let Some(idx) = data.windows(4).position(|w| w == *b"ftyp") {
        if idx >= 4 {
            // The 4 bytes immediately before "ftyp" are the box size.
            return Some((InputFormat::Mp4, idx - 4));
        }
    }
    None
}

/// Spawn ffmpeg configured for the detected input container. WebM is
/// transcoded to H.264/AAC; fragmented MP4 from MediaRecorder is already
/// H.264/AAC so we copy both streams to skip an entire encode pass.
fn spawn_ffmpeg(
    format: InputFormat,
    destination: &str,
) -> std::io::Result<tokio::process::Child> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-fflags",
        "nobuffer+genpts",
        "-flags",
        "low_delay",
        "-probesize",
        "32K",
        "-analyzeduration",
        "0",
    ]);

    match format {
        InputFormat::WebM => {
            cmd.args([
                "-f",
                "webm",
                "-i",
                "pipe:0",
                // libx264 with yuv420p requires both dimensions to be even.
                // The browser canvas (e.g. a resized fidget-spinner element)
                // can produce odd widths/heights like 761x877, which causes
                // ffmpeg to abort with "width not divisible by 2" and tear
                // down the whole ingest. Round both dims down to the nearest
                // even number and pin the pixel format so the encoder always
                // gets a valid input regardless of the source resolution.
                "-vf",
                "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
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
                // Pin profile + sample rate + channels so Chromium's MSE AAC
                // decoder doesn't reject the stream with kUnsupportedConfig.
                "-profile:a",
                "aac_low",
                "-ar",
                "48000",
                "-ac",
                "2",
                "-b:a",
                "128k",
            ]);
        }
        InputFormat::Mp4 => {
            // MediaRecorder fMP4 is already H.264/AAC LC at the right
            // sample rate; copy both streams straight through. This is
            // the big latency win: no encoder lookahead, no GOP rewrite.
            cmd.args([
                "-f",
                "mp4",
                "-i",
                "pipe:0",
                "-c:v",
                "copy",
                "-c:a",
                "copy",
                "-bsf:a",
                "aac_adtstoasc",
            ]);
        }
    }

    cmd.args([
        "-f",
        "flv",
        "-flvflags",
        "no_duration_filesize",
        destination,
    ])
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::null())
    .stderr(std::process::Stdio::piped())
    .spawn()
}
