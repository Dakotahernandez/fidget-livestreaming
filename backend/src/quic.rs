use std::sync::{Arc, RwLock};
use std::time::Duration;

use sqlx::PgPool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;
use wtransport::{Endpoint, Identity, ServerConfig};

use crate::middleware::auth::{decode_chat_access_token, Claims};
use crate::models::stream;
use crate::quic_chat;

//Read buffer for the unidirectional ingest stream. Sized to swallow one
//MediaRecorder timeslice (~100 ms of VP8/Opus) without thrashing read().
const STREAM_READ_BUF: usize = 64 * 1024;

/// How often the QUIC self-signed identity is regenerated. Chrome's
/// WebTransport spec rejects `serverCertificateHashes` certificates whose
/// validity period exceeds 14 days, AND of course rejects expired ones,
/// so the previous "build once at startup" design silently broke browser
/// ingest after two weeks of uptime (OBS / RTMP kept working, hence the
/// long-running deploy bug). Rotating well under that ceiling keeps
/// browser publishers reliably connected.
const CERT_ROTATION_INTERVAL: Duration = Duration::from_secs(60 * 60 * 24 * 10);

/// Build a fresh self-signed identity for the WebTransport endpoint.
fn build_identity() -> Identity {
    Identity::self_signed(["localhost", "127.0.0.1", "::1"])
        .expect("failed to create self-signed identity")
}

/// Compute the hex-encoded SHA-256 hash of the leaf certificate so the
/// frontend can pin it via `serverCertificateHashes`.
fn cert_hash_hex(identity: &Identity) -> String {
    let leaf = identity
        .certificate_chain()
        .as_slice()
        .first()
        .expect("empty certificate chain");
    hex::encode(leaf.hash().as_ref())
}

/// Start the QUIC/WebTransport ingest server and return its SHA-256 cert hash.
pub fn start_quic_server(
    port: u16,
    jwt_secret: String,
    rtmp_url: String,
    pool: PgPool,
    chat_state: quic_chat::ChatState,
) -> Arc<RwLock<String>> {
    // Build the first identity synchronously so the HTTP server can serve
    // a valid hash immediately, even before the rotation task runs.
    let initial_identity = build_identity();
    let initial_hash = cert_hash_hex(&initial_identity);
    let cert_hash = Arc::new(RwLock::new(initial_hash.clone()));

    tracing::info!(
        "QUIC ingest server starting on UDP port {port} (cert hash: {initial_hash})"
    );

    //3s keep-alive pings stop NATs and CGNAT firewalls from idling the QUIC connection.
    let cert_hash_for_task = cert_hash.clone();

    //Hand off the accept loop to a background task and return so main() can
    //finish bringing up the HTTP router.
    tokio::spawn(async move {
        let mut current_identity = initial_identity;
        loop {
            let config = ServerConfig::builder()
                .with_bind_default(port)
                .with_identity(current_identity)
                .keep_alive_interval(Some(Duration::from_secs(3)))
                .build();

            // Race the accept loop against the rotation timer. When the
            // timer fires `tokio::select!` drops the accept loop future,
            // which drops the `Endpoint` and releases the UDP port so we
            // can immediately rebind with a fresh identity. Active
            // WebTransport sessions are torn down by this — a brief
            // disruption every ~10 days is acceptable for the academic
            // deployment.
            tokio::select! {
                _ = run_quic_server(
                    config,
                    jwt_secret.clone(),
                    rtmp_url.clone(),
                    pool.clone(),
                    chat_state.clone(),
                ) => {
                    tracing::warn!(
                        "QUIC accept loop exited unexpectedly; restarting in 5s"
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
                _ = tokio::time::sleep(CERT_ROTATION_INTERVAL) => {
                    tracing::info!("Rotating QUIC self-signed certificate");
                }
            }

            current_identity = build_identity();
            let new_hash = cert_hash_hex(&current_identity);
            tracing::info!("New QUIC cert hash: {new_hash}");
            *cert_hash_for_task
                .write()
                .expect("cert hash lock poisoned") = new_hash;
        }
    });

    cert_hash
}

/// Accept loop - one task per WebTransport session.
async fn run_quic_server(
    config: ServerConfig,
    jwt_secret: String,
    rtmp_url: String,
    pool: PgPool,
    chat_state: quic_chat::ChatState
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
        let chat_state = chat_state.clone();

        tokio::spawn(async move {
            let session_request = match incoming_session.await {
                Ok(req) => req,
                Err(e) => {
                    tracing::warn!("WebTransport session error: {e}");
                    return;
                }
            };

            //Path looks like /ingest/<uuid>?token=<jwt> for video, /chat/<uuid>?token=<jwt> for chat.
            let path = session_request.path().to_string();

            //Chat sessions get routed off to quic_chat. Everything else falls through to video ingest.
            if let Some((stream_id, token)) = quic_chat::parse_chat_path(&path) {
                let chat_claims = match decode_chat_access_token(&token, &jwt_secret) {
                    Ok(claims) => claims,
                    Err(_) => {
                        tracing::warn!("Invalid chat token for stream {stream_id}");
                        return;
                    }
                };

                if chat_claims.stream_id != stream_id {
                    tracing::warn!(
                        "Rejected chat token with mismatched stream claim (claim={}, path={})",
                        chat_claims.stream_id,
                        stream_id
                    );
                    return;
                }

                let user_id = chat_claims.sub;

                match stream::find_by_id(&pool, stream_id).await {
                    Ok(Some(_)) => {}
                    _ => {
                        tracing::warn!("Chat stream {stream_id} not found");
                        return;
                    }
                }

                let connection = match session_request.accept().await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!("Failed to accept WebTransport chat session: {e}");
                        return;
                    }
                };

                tracing::info!(
                    "WebTransport chat session accepted for stream {stream_id} from {}",
                    connection.remote_address()
                );

                quic_chat::handle_chat_session(
                    connection,
                    pool,
                    stream_id,
                    user_id,
                    chat_state,
                )
                .await;

                return;
            }

            //Video ingest path.
            let (stream_id, token) = match parse_ingest_path(&path) {
                Some(v) => v,
                None => {
                    tracing::warn!("Invalid ingest path: {path}");
                    return;
                }
            };

            //JWT check.
            let user_id = match validate_token(&token, &jwt_secret) {
                Some(id) => id,
                None => {
                    tracing::warn!("Invalid token for stream {stream_id}");
                    return;
                }
            };

            //Owner check - only the row's owner can push to it.
            let s = match stream::find_by_id(&pool, stream_id).await {
                Ok(Some(s)) if s.user_id == user_id => s,
                _ => {
                    tracing::warn!("Stream {stream_id} not found or not owned by user {user_id}");
                    return;
                }
            };

            //Auth passed - accept and start ingest.
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

/// Parse `/ingest/<uuid>?token=<jwt>` into (stream_id, token).
fn parse_ingest_path(path: &str) -> Option<(Uuid, String)> {
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

/// Decode the JWT and return its subject (user_id) on success.
fn validate_token(token: &str, secret: &str) -> Option<Uuid> {
    let token_data = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )
    .ok()?;

    Some(token_data.claims.sub)
}

/// Drive one ingest session: read the WT stream and pipe it into ffmpeg.
async fn handle_session(
    connection: wtransport::Connection,
    pool: PgPool,
    stream_id: Uuid,
    stream_key: String,
    rtmp_url: String,
) {
    //Flip the row to is_live = true so the home page picks it up.
    if let Err(e) = stream::set_live(&pool, stream_id).await {
        tracing::error!("Failed to mark stream {stream_id} as live: {e}");
        return;
    }

    let destination = format!("{rtmp_url}/{stream_key}");
    tracing::info!("Starting WebTransport ingest for stream {stream_id} → {destination}");

    //One ordered unidirectional stream carries the whole WebM byte stream.
    //We tried QUIC datagrams first - Firefox silently dropped them.
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

    // Optimization
    //Sniff the container format before spawning ffmpeg. MP4 + H.264 lets us
    //`-c:v copy` and dodge the 200-400 ms libx264 transcode penalty.
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

    //Pump ffmpeg's stderr into tracing. If we don't drain it the pipe buffer fills and encoding stalls.
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::info!(stream_id = %stream_id, "ffmpeg: {line}");
            }
        });
    }

    //Replay the buffered prefix (skipping any priming bytes before the real header) into ffmpeg.
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

    //Tear down ffmpeg.
    drop(stdin);
    let _ = child.kill().await;
    let _ = child.wait().await;

    //Hard-delete the stream row. Going live again creates a fresh one.
    if let Err(e) = stream::set_offline(&pool, stream_id).await {
        tracing::error!("Failed to mark stream {stream_id} offline: {e}");
    }

    tracing::info!("Ingest ended for stream {stream_id}");
}

//What the browser is shipping. Decides whether ffmpeg has to transcode.
#[derive(Debug, Clone, Copy)]
enum InputFormat {
    WebM,  //VP8 + Opus, Matroska container. Firefox + older Chrome.
    Mp4,   //H.264 + AAC, fragmented MP4. Chrome >= 130.
}

/// Sniff the container from the leading bytes. Returns `(fmt, offset)` where
/// `offset` is where the real header begins, so the caller can drop any
/// priming bytes MediaRecorder emits before the first cluster/box.
fn detect_input_format(data: &[u8]) -> Option<(InputFormat, usize)> {
    const EBML_MAGIC: [u8; 4] = [0x1A, 0x45, 0xDF, 0xA3];

    if let Some(idx) = data.windows(4).position(|w| w == EBML_MAGIC) {
        return Some((InputFormat::WebM, idx));
    }
    if let Some(idx) = data.windows(4).position(|w| w == *b"ftyp") {
        if idx >= 4 {
            //The 4 bytes right before "ftyp" are the box size - that's where the box actually starts.
            return Some((InputFormat::Mp4, idx - 4));
        }
    }
    None
}

//Quality ladder. Each entry is a separate RTMP publish → SRS makes a
//matching HLS playlist per rendition. Viewers append the suffix to the
//stream key to swap renditions: `…/hls/live_<uuid>_720p/index.m3u8`.
//
//Resolutions are an upper bound - the scale filter uses
//`force_original_aspect_ratio=decrease`, so a smaller source passes
//through at its native size rather than upscaling. Bitrates are tuned
//for ~30 fps screen-capture with libx264 veryfast.
const RENDITIONS: &[Rendition] = &[
    Rendition { suffix: "_1080p", width: 1920, height: 1080, video_kbps: 4000, audio_kbps: 128 },
    Rendition { suffix: "_720p",  width: 1280, height: 720,  video_kbps: 2000, audio_kbps: 96  },
    Rendition { suffix: "_360p",  width: 640,  height: 360,  video_kbps: 600,  audio_kbps: 64  },
];

#[derive(Debug, Clone, Copy)]
struct Rendition {
    suffix: &'static str,  //stream-key suffix. Empty = source.
    width: u32,            //target width. 0 = keep source.
    height: u32,           //target height. 0 = keep source.
    video_kbps: u32,       //video bitrate in kbps. 0 = let libx264 pick.
    audio_kbps: u32,       //audio bitrate in kbps.
}

/// Spawn ffmpeg to demux the input pipe and fanout to every rendition.

fn spawn_ffmpeg(
    format: InputFormat,
    destination: &str,
) -> std::io::Result<tokio::process::Child> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-fflags",
        "nobuffer+genpts+discardcorrupt",
        "-flags",
        "low_delay",
        "-err_detect",
        "ignore_err",
        "-probesize",
        "32K",
        "-analyzeduration",
        "0",
    ]);

    //Input container.
    match format {
        InputFormat::WebM => {
            cmd.args(["-f", "webm", "-i", "pipe:0"]);
        }
        InputFormat::Mp4 => {
            cmd.args(["-f", "mp4", "-i", "pipe:0"]);
        }
    }

    //Which renditions join the libx264 filter graph?
    //WebM source must re-encode (VP8 → H.264) -> joins.
    //All downscales always join.
    let source_through_filter = matches!(format, InputFormat::WebM);
    let mut filter_renditions: Vec<&Rendition> = Vec::new();
    if source_through_filter {
        if let Some(src) = RENDITIONS.iter().find(|r| r.suffix.is_empty()) {
            filter_renditions.push(src);
        }
    }
    for r in RENDITIONS.iter().filter(|r| !r.suffix.is_empty()) {
        filter_renditions.push(r);
    }

    //Build the `-filter_complex` graph. Even a single branch goes through
    //`split=1` so every rendition ends up with a labelled output.
    let pad_label = |suffix: &str| -> String {
        if suffix.is_empty() {
            "v_src".to_string()
        } else {
            format!("v{}", suffix)
        }
    };
    let labels: Vec<String> = filter_renditions
        .iter()
        .map(|r| pad_label(r.suffix))
        .collect();

    if !filter_renditions.is_empty() {
        let n = filter_renditions.len();
        let mut graph = String::new();
        //Fork the decoded video N ways.
        graph.push_str("[0:v]split=");
        graph.push_str(&n.to_string());
        for i in 0..n {
            graph.push_str(&format!("[s{i}]"));
        }
        graph.push(';');
        //Per-branch scaling.
        for (i, r) in filter_renditions.iter().enumerate() {
            let chain = if r.suffix.is_empty() {
                //Source: just enforce even dims + pixel format.
                "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p".to_string()
            } else {
                //Downscale: fit-within target box, then even dims + pix fmt.
                format!(
                    "scale={}:{}:force_original_aspect_ratio=decrease,scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p",
                    r.width, r.height
                )
            };
            graph.push_str(&format!("[s{i}]{chain}[{}];", labels[i]));
        }
        //Strip trailing ';'.
        graph.pop();
        cmd.args(["-filter_complex", graph.as_str()]);
    }

    for r in RENDITIONS {
        let is_source = r.suffix.is_empty();

        let through_filter = filter_renditions.iter().any(|fr| fr.suffix == r.suffix);
        let map_label = if through_filter {
            format!("[{}]", pad_label(r.suffix))
        } else {
            "0:v".to_string()
        };
        cmd.args(["-map", map_label.as_str(), "-map", "0:a"]);

        let needs_libx264 = match format {
            InputFormat::WebM => true,
            InputFormat::Mp4 => !is_source,
        };
        if needs_libx264 {
            cmd.args([
                "-c:v", "libx264",
                "-pix_fmt", "yuv420p",
                "-preset", "veryfast",
                "-tune", "zerolatency",
                "-g", "30",
                "-keyint_min", "30",
                "-sc_threshold", "0",
                "-force_key_frames", "expr:gte(t,n_forced*1)",
            ]);
            if r.video_kbps > 0 {
                let b = format!("{}k", r.video_kbps);
                let maxb = format!("{}k", r.video_kbps * 3 / 2);
                let bufb = format!("{}k", r.video_kbps * 4);
                cmd.args([
                    "-b:v", b.as_str(),
                    "-maxrate", maxb.as_str(),
                    "-bufsize", bufb.as_str(),
                ]);
            }
        } else {
            cmd.args(["-c:v", "copy"]);
        }

        if !is_source || matches!(format, InputFormat::WebM) {
            let b = format!("{}k", r.audio_kbps);
            cmd.args([
                "-c:a", "aac",
                "-profile:a", "aac_low",
                "-ar", "48000",
                "-ac", "2",
                "-b:a", b.as_str(),
            ]);
        } else {
            cmd.args(["-c:a", "copy", "-bsf:a", "aac_adtstoasc"]);
        }

        let out_url = if is_source {
            destination.to_string()
        } else {
            format!("{destination}{}", r.suffix)
        };
        cmd.args(["-f", "flv", "-flvflags", "no_duration_filesize"]);
        cmd.arg(out_url.as_str());
    }

    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
}
