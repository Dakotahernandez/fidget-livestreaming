//! Multi-rendition HLS ladder fan-out for the WHIP / SRS-driven ingest path.
//!
//! When a streamer publishes via WHIP, SRS receives the source over RTC,
//! bridges it into RTMP (`rtmp://srs:1935/live/<key>`), and produces the
//! source HLS playlist. The viewer-side quality dropdown in `Watch.jsx`
//! also expects `<key>_720p` and `<key>_360p` playlists. This module
//! supplies them by spawning an `ffmpeg` process that pulls the source
//! RTMP stream from SRS and pushes back one rendition per ladder entry.
//!
//! The WebTransport ingest path in `quic.rs` runs its own `ffmpeg`
//! pipeline (it owns the raw browser bitstream) and is intentionally
//! left untouched — the two ingest modes share only the high-level
//! rendition definition, not the encoder invocation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// One entry in the HLS quality ladder produced from the WHIP source.
#[derive(Debug, Clone, Copy)]
pub struct WhipRendition {
    pub suffix: &'static str, //RTMP / HLS stream-key suffix.
    pub width: u32,
    pub height: u32,
    pub video_kbps: u32,
    pub audio_kbps: u32,
}

/// Renditions to derive from the WHIP source. The bare `<key>` playlist
/// (the SRS-native HLS that already exists for the source publish) acts
/// as the "Source" tier on the viewer side, so the ladder here only needs
/// the lower tiers.
pub const WHIP_LADDER: &[WhipRendition] = &[
    WhipRendition { suffix: "_720p", width: 1280, height: 720, video_kbps: 2000, audio_kbps: 96 },
    WhipRendition { suffix: "_360p", width: 640,  height: 360, video_kbps: 600,  audio_kbps: 64 },
];

/// Process registry keyed by the source stream id. Cloned into Axum
/// router state so both `on_publish` and `on_publish_done` can reach it.
pub type LadderRegistry = Arc<Mutex<HashMap<Uuid, JoinHandle<()>>>>;

/// Build a fresh, empty registry. Call once at startup.
pub fn new_registry() -> LadderRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

/// How long to wait between `ffmpeg` restart attempts when the child
/// exits unexpectedly mid-stream (transient SRS hiccup, network blip).
const RESTART_BACKOFF: Duration = Duration::from_secs(2);

/// Cap on total restart attempts per `start_ladder` invocation. Prevents
/// a permanently-broken source from spinning forever in the background.
const MAX_RESTARTS: u32 = 8;

/// Grace period before the very first ffmpeg attempt. SRS fires
/// `on_publish` the instant the WHIP / RTC publish is accepted, but its
/// internal `rtc_to_rtmp` bridge needs a moment to start republishing
/// the stream as RTMP. Without this delay ffmpeg connects to an empty
/// source and the supervisor burns through restart attempts before the
/// bridge is actually ready.
const INITIAL_DELAY: Duration = Duration::from_millis(1500);

/// Spawn (and supervise) the rendition ladder for the given stream.
///
/// `rtmp_base` is the prefix used for both source pull and rendition
/// push, e.g. `rtmp://srs:1935/live`. The function returns immediately;
/// the supervisor task is registered under `stream_id` so a later
/// `stop_ladder` call can reap it.
///
/// If a ladder is already running for `stream_id` it is replaced — the
/// previous task is aborted before the new one is spawned so we never
/// have two concurrent encoders for the same key.
pub async fn start_ladder(
    registry: &LadderRegistry,
    stream_id: Uuid,
    rtmp_base: String,
    stream_key: String,
) {
    //Replace any prior supervisor for this stream id.
    if let Some(prev) = registry.lock().await.remove(&stream_id) {
        prev.abort();
    }

    let registry_clone = registry.clone();
    let key = stream_key.clone();
    let base = rtmp_base.clone();

    let handle = tokio::spawn(async move {
        supervise(stream_id, base, key).await;
        //When the supervisor returns naturally (max restarts hit) drop our
        //own slot so a future publish can spawn cleanly.
        registry_clone.lock().await.remove(&stream_id);
    });

    registry.lock().await.insert(stream_id, handle);
}

/// Tear down the ladder for a stream. No-op if nothing is running.
pub async fn stop_ladder(registry: &LadderRegistry, stream_id: Uuid) {
    if let Some(handle) = registry.lock().await.remove(&stream_id) {
        handle.abort();
    }
}

/// Long-running supervisor: spawns `ffmpeg`, waits for it, restarts on
/// unexpected exit up to `MAX_RESTARTS` times.
async fn supervise(stream_id: Uuid, rtmp_base: String, stream_key: String) {
    //Give SRS's rtc_to_rtmp bridge a moment to start republishing the
    //WHIP source as RTMP before the first ffmpeg pull attempt.
    tokio::time::sleep(INITIAL_DELAY).await;

    let mut restarts: u32 = 0;
    loop {
        let source = format!("{rtmp_base}/{stream_key}");
        let mut child = match spawn_whip_ladder_ffmpeg(&rtmp_base, &stream_key) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(
                    stream_id = %stream_id,
                    source = %source,
                    "Failed to spawn rendition ffmpeg: {e}"
                );
                return;
            }
        };

        tracing::info!(
            stream_id = %stream_id,
            source = %source,
            renditions = WHIP_LADDER.len(),
            "Spawned WHIP rendition ladder"
        );

        let exit = child.wait().await;
        match exit {
            Ok(status) if status.success() => {
                //Source publish ended cleanly (publisher disconnected, SRS
                //tore down the stream). Don't bother restarting.
                tracing::info!(
                    stream_id = %stream_id,
                    "Rendition ffmpeg exited cleanly"
                );
                return;
            }
            Ok(status) => {
                tracing::warn!(
                    stream_id = %stream_id,
                    code = ?status.code(),
                    "Rendition ffmpeg exited with non-zero status"
                );
            }
            Err(e) => {
                tracing::warn!(
                    stream_id = %stream_id,
                    "Failed to wait on rendition ffmpeg: {e}"
                );
            }
        }

        restarts += 1;
        if restarts > MAX_RESTARTS {
            tracing::error!(
                stream_id = %stream_id,
                "Giving up on rendition ffmpeg after {MAX_RESTARTS} restarts"
            );
            return;
        }
        tracing::info!(
            stream_id = %stream_id,
            attempt = restarts,
            "Restarting rendition ffmpeg after backoff"
        );
        tokio::time::sleep(RESTART_BACKOFF).await;
    }
}

/// Build the `ffmpeg` invocation that pulls the source RTMP and pushes
/// one libx264 + AAC rendition per `WHIP_LADDER` entry back to SRS.
///
/// Audio is re-encoded (cheap) to keep one consistent profile across
/// renditions; this also avoids tracking SRS's exact ADTS framing
/// across version bumps.
fn spawn_whip_ladder_ffmpeg(
    rtmp_base: &str,
    stream_key: &str,
) -> std::io::Result<Child> {
    let source = format!("{rtmp_base}/{stream_key}");

    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-hide_banner",
        //`info` so the supervisor's stderr-inherit pipes useful frame /
        //connection diagnostics into the backend container logs while we
        //iterate. Drop back to `warning` once the WHIP path is stable.
        "-loglevel", "info",
        //Low-latency input flags. Note: `-reconnect*` is HTTP-only and is
        //rejected by ffmpeg when the input demuxer is `flv` (RTMP), so
        //transient SRS disconnects are handled by the supervisor's
        //restart loop instead of a per-input reconnect option.
        "-fflags", "nobuffer+genpts",
        "-flags", "low_delay",
        "-f", "flv",
        "-i", source.as_str(),
    ]);

    //Build `[0:v]split=N[s0][s1]...; [s0]scale=...[v_720p]; ...` filter graph.
    let n = WHIP_LADDER.len();
    let mut graph = String::new();
    graph.push_str("[0:v]split=");
    graph.push_str(&n.to_string());
    for i in 0..n {
        graph.push_str(&format!("[s{i}]"));
    }
    graph.push(';');
    for (i, r) in WHIP_LADDER.iter().enumerate() {
        graph.push_str(&format!(
            "[s{i}]scale={}:{}:force_original_aspect_ratio=decrease,scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p[v{}];",
            r.width, r.height, r.suffix
        ));
    }
    graph.pop(); //strip trailing ';'
    cmd.args(["-filter_complex", graph.as_str()]);

    for r in WHIP_LADDER {
        let map_label = format!("[v{}]", r.suffix);
        cmd.args(["-map", map_label.as_str(), "-map", "0:a"]);

        cmd.args([
            "-c:v", "libx264",
            "-pix_fmt", "yuv420p",
            "-preset", "veryfast",
            "-tune", "zerolatency",
            "-profile:v", "baseline",
            "-level", "3.1",
            "-vsync", "cfr",
            "-r", "30",
            //GOP = 0.5s to align with `hls_fragment 0.5` in srs.conf so
            //every HLS segment begins on an IDR.
            "-g", "15",
            "-keyint_min", "15",
            "-sc_threshold", "0",
            "-force_key_frames", "expr:gte(t,n_forced*0.5)",
        ]);
        let b = format!("{}k", r.video_kbps);
        let maxb = format!("{}k", r.video_kbps * 3 / 2);
        let bufb = format!("{}k", r.video_kbps * 4);
        cmd.args([
            "-b:v", b.as_str(),
            "-maxrate", maxb.as_str(),
            "-bufsize", bufb.as_str(),
        ]);

        let ab = format!("{}k", r.audio_kbps);
        cmd.args([
            "-c:a", "aac",
            "-profile:a", "aac_low",
            "-ar", "48000",
            "-ac", "2",
            "-b:a", ab.as_str(),
        ]);

        let out_url = format!("{rtmp_base}/{stream_key}{}", r.suffix);
        cmd.args(["-f", "flv", "-flvflags", "no_duration_filesize"]);
        cmd.arg(out_url.as_str());
    }

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        //Inherit stderr so ffmpeg warnings/errors show up directly in
        //the backend container logs alongside our supervisor messages.
        //That makes diagnosing source-stream issues (lost connection,
        //bad codec params) a single `docker compose logs backend` away.
        .stderr(std::process::Stdio::inherit())
        //Ensure the OS reaps the encoder when our supervisor task is
        //aborted (e.g. via `stop_ladder` on `on_publish_done`). Without
        //this, dropping the `Child` only detaches it and the ffmpeg
        //process keeps running as an orphan inside the backend container.
        .kill_on_drop(true)
        .spawn()
}
