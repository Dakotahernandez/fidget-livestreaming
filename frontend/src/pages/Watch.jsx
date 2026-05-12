import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useParams } from 'react-router-dom';
import Hls from 'hls.js';
import { getStream, joinStream, resolvePlaybackRoute } from '../services/api';
import StreamChat from '../components/chat/StreamChat';

// Manual quality selection ladder. Values match the rendition suffixes
// the backend's ffmpeg pushes to SRS. The 'source' value selects the
// untouched publish (no suffix on the HLS URL); other values map to
// `<streamKey>_<value>`. The set is intentionally small — WHIP-sourced
// streams negotiate a single SDP resolution that becomes "Source", so
// offering a separate 1080p tier above it would be wasteful upscaling.
const QUALITY_OPTIONS = [
  { value: 'source', label: 'Source' },
  { value: '720p',   label: '720p' },
  { value: '360p',   label: '360p' },
];
const DEFAULT_QUALITY = 'source';

const EMPTY_PLAYBACK_METRICS = {
  source: '',
  edgeId: '',
  routeLookupMs: null,
  manifestLoadMs: null,
  firstFrameMs: null,
  liveLatencyMs: null,
  bufferMs: null,
};

// WHEP signalling is proxied same-origin through the frontend nginx at
// `/whep/` (see frontend/nginx.conf). That keeps it HTTPS-clean in prod
// (no mixed content), avoids CORS preflights, and means the browser
// never has to know SRS's port directly.
const WHEP_PATH = '/whep/';
const WHEP_APP = 'live';
// How long we wait for the WHEP peer connection to deliver its first
// video frame before giving up and falling back to HLS. Kept short so a
// blocked UDP path (e.g. EC2 SG missing UDP 8000) doesn't leave the
// viewer staring at a black player for seconds before HLS takes over.
const WHEP_FIRST_FRAME_TIMEOUT_MS = 1500;

/// Fetch the deployment-specific ICE server list from the backend. Falls
/// back to a public STUN-only config so first-time deployments still get
/// host-candidate WHEP working without extra setup.
async function fetchIceServers(signal) {
  try {
    const res = await fetch('/api/config/rtc', { signal });
    if (res.ok) {
      const body = await res.json();
      if (Array.isArray(body?.ice_servers)) return body.ice_servers;
    }
  } catch { /* fall through to default */ }
  return [{ urls: 'stun:stun.l.google.com:19302' }];
}

/// Attempt sub-second playback via WHEP (WebRTC). Resolves with a
/// `cleanup` function once the first frame is rendered. Rejects on
/// signalling errors, ICE failure, or if no frame arrives in time so the
/// caller can transparently fall back to HLS.
async function startWhepPlayback({ streamKey, video, signal }) {
  const iceServers = await fetchIceServers(signal);
  const whepUrl = `${WHEP_PATH}?app=${WHEP_APP}&stream=${encodeURIComponent(streamKey)}`;

  const pc = new RTCPeerConnection({
    bundlePolicy: 'max-bundle',
    iceServers,
  });

  pc.addTransceiver('video', { direction: 'recvonly' });
  pc.addTransceiver('audio', { direction: 'recvonly' });

  // Wire received tracks to the <video> element as they arrive.
  // Use event.track directly rather than event.streams[0]?.getTracks() —
  // SRS may send tracks ungrouped (empty streams array), which would
  // silently add nothing and leave the video element with no video track
  // (audio plays but video stays black).
  const remoteStream = new MediaStream();
  video.srcObject = remoteStream;
  video.autoplay = true;
  video.playsInline = true;
  pc.ontrack = (event) => {
    remoteStream.addTrack(event.track);
  };

  const cleanup = () => {
    try { pc.close(); } catch { /* ignore */ }
    if (video.srcObject === remoteStream) {
      video.srcObject = null;
    }
  };

  if (signal?.aborted) {
    cleanup();
    throw new Error('aborted');
  }
  signal?.addEventListener('abort', cleanup, { once: true });

  const offer = await pc.createOffer();
  await pc.setLocalDescription(offer);

  const response = await fetch(whepUrl, {
    method: 'POST',
    headers: { 'Content-Type': 'application/sdp' },
    body: offer.sdp,
    signal,
  });
  if (!response.ok) {
    cleanup();
    throw new Error(`WHEP signalling failed: ${response.status}`);
  }
  const answerSdp = await response.text();
  await pc.setRemoteDescription({ type: 'answer', sdp: answerSdp });

  // Resolve only when an actual video frame has rendered — not just on
  // the `playing` event, which fires as soon as any media (audio
  // included) starts rendering. If the audio decodes but the video
  // doesn't (codec/profile mismatch, decoder rejection), the old logic
  // resolved successfully on the audio-only `playing` event and HLS
  // fallback never ran — the user saw audio with a black video element.
  // Prefer requestVideoFrameCallback for an authoritative signal, and
  // fall back to polling `videoWidth` when the API isn't available.
  await new Promise((resolve, reject) => {
    let settled = false;
    let timer = null;
    let pollTimer = null;

    const finish = (err) => {
      if (settled) return;
      settled = true;
      if (timer) clearTimeout(timer);
      if (pollTimer) clearInterval(pollTimer);
      if (err) reject(err); else resolve();
    };

    timer = setTimeout(
      () => finish(new Error('WHEP first-video-frame timeout')),
      WHEP_FIRST_FRAME_TIMEOUT_MS,
    );

    if (typeof video.requestVideoFrameCallback === 'function') {
      video.requestVideoFrameCallback(() => finish());
    } else {
      pollTimer = setInterval(() => {
        if (video.videoWidth > 0 && video.readyState >= 2) finish();
      }, 100);
    }

    // Some browsers need an explicit play() after srcObject changes.
    video.play().catch(() => { /* ignore — frame callback still fires */ });
  });

  return cleanup;
}

/**
 * Watch page — plays a live stream via WHEP (WebRTC) when available,
 * falling back to low-latency HLS.
 */
function Watch() {
  const { id } = useParams();
  const videoRef = useRef(null);
  const hlsRef = useRef(null);

  const [stream, setStream] = useState(null);
  const [hlsUrl, setHlsUrl] = useState('');
  const [streamKey, setStreamKey] = useState('');
  const [chatToken, setChatToken] = useState('');
  const [error, setError] = useState('');
  const [playerError, setPlayerError] = useState('');
  const [joinPassword, setJoinPassword] = useState('');
  const [joinError, setJoinError] = useState('');
  const [joining, setJoining] = useState(false);
  const [playbackMetrics, setPlaybackMetrics] = useState(EMPTY_PLAYBACK_METRICS);
  const [quality, setQuality] = useState(DEFAULT_QUALITY);

  // Derived playback identifiers that fold the user's quality choice into
  // the URL/stream-key. The player effect keys on these so changing the
  // dropdown tears down the current WHEP/HLS player and starts a fresh
  // one against the new rendition. The 'source' tier is the untouched
  // publish key (no suffix); every other tier appends `_<quality>`.
  const playbackKey = useMemo(
    () => {
      if (!streamKey) return streamKey;
      return quality === 'source' ? streamKey : `${streamKey}_${quality}`;
    },
    [streamKey, quality],
  );

  const playbackUrl = useMemo(() => {
    if (!hlsUrl || !streamKey) return hlsUrl;
    if (quality === 'source') return hlsUrl;
    const nextKey = `${streamKey}_${quality}`;
    // Both origin and edge-routed URLs end in `…/hls/<streamKey>/index.m3u8`,
    // so it's safe to swap just the key segment without re-running the
    // route lookup.
    return hlsUrl.replace(`/${streamKey}/`, `/${nextKey}/`);
  }, [hlsUrl, streamKey, quality]);

  const updatePlaybackMetrics = useCallback((partial) => {
    setPlaybackMetrics((current) => ({ ...current, ...partial }));
  }, []);

  const unlockStream = useCallback(async (password = '') => {
    try {
      const startedAt = performance.now();
      setJoining(true);
      setJoinError('');
      setPlayerError('');
      setPlaybackMetrics(EMPTY_PLAYBACK_METRICS);

      const res = await joinStream(id, password);
      const fallbackUrl = res.data?.hls_url || '';
      const nextStreamKey = res.data?.stream_key || '';
      const nextChatToken = res.data?.chat_access_token || '';

      let nextUrl = fallbackUrl;
      let playbackSource = 'origin';
      let edgeId = '';

      if (nextStreamKey) {
        try {
          const routeRes = await resolvePlaybackRoute(nextStreamKey);
          if (routeRes.data?.edge_url) {
            nextUrl = routeRes.data.edge_url;
            playbackSource = 'edge';
            edgeId = routeRes.data?.edge_id || '';
          }
        } catch {
          playbackSource = 'origin-fallback';
        }
      }

      updatePlaybackMetrics({
        source: playbackSource,
        edgeId,
        routeLookupMs: Math.round(performance.now() - startedAt),
      });
      setStreamKey(nextStreamKey);
      setHlsUrl(nextUrl);
      setChatToken(nextChatToken);
    } catch (err) {
      const msg = err.response?.data?.error || 'Unable to join this stream.';
      setJoinError(msg);
      setHlsUrl('');
      setChatToken('');
    } finally {
      setJoining(false);
    }
  }, [id, updatePlaybackMetrics]);

  // Fetch stream details from the backend.
  useEffect(() => {
    let cancelled = false;

    const fetchStream = async () => {
      try {
        const res = await getStream(id);
        if (!cancelled) {
          setStream(res.data);
          if (!res.data?.requires_password) {
            unlockStream('');
          }
        }
      } catch {
        if (!cancelled) setError('Stream not found.');
      }
    };

    fetchStream();
    return () => {
      cancelled = true;
    };
  }, [id, unlockStream]);

  // Attach a player when a stream URL is available. WHEP is tried first
  // for sub-second latency; HLS is the always-available fallback. The
  // effect keys on the playback (rendition-aware) URL/key, so changing
  // the quality dropdown tears down and re-attaches the player against
  // the new rendition.
  useEffect(() => {
    if (!playbackUrl || !videoRef.current) return;

    const video = videoRef.current;
    const playbackStartedAt = performance.now();
    let metricTimer = null;
    let manifestRetryTimer = null;
    let whepCleanup = null;
    let whepAbort = null;
    let cancelled = false;

    const updateBufferMetrics = (currentHls = hlsRef.current) => {
      const nextMetrics = {};

      if (currentHls && Number.isFinite(currentHls.latency)) {
        nextMetrics.liveLatencyMs = Math.round(currentHls.latency * 1000);
      }

      if (video.buffered.length > 0) {
        const bufferedEnd = video.buffered.end(video.buffered.length - 1);
        nextMetrics.bufferMs = Math.max(
          0,
          Math.round((bufferedEnd - video.currentTime) * 1000),
        );
      }

      if (Object.keys(nextMetrics).length > 0) {
        updatePlaybackMetrics(nextMetrics);
      }
    };

    const handlePlaying = () => {
      updatePlaybackMetrics({
        firstFrameMs: Math.round(performance.now() - playbackStartedAt),
      });
    };

    video.addEventListener('playing', handlePlaying);
    setPlayerError('');
    updatePlaybackMetrics({
      manifestLoadMs: null,
      firstFrameMs: null,
      liveLatencyMs: null,
      bufferMs: null,
    });

    const startHls = () => {
      if (cancelled) return;
      if (Hls.isSupported()) {
        // Tuned for SRS-served HLS with 0.5s fragments. Targets ~1.5s
        const hls = new Hls({
          enableWorker: true,
          lowLatencyMode: true,
          liveSyncDuration: 1,
          liveMaxLatencyDuration: 3,
          maxBufferLength: 2,
          maxMaxBufferLength: 4,
          backBufferLength: 4,
          maxLiveSyncPlaybackRate: 1.5,
          // SRS doesn't emit a CODECS attribute in its master playlist, so
          // pin AAC LC explicitly. Without this, hls.js may guess HE-AAC
          // (mp4a.40.5) and Chromium's MSE rejects the source buffer with
          // "audio decoder initialization failed: kUnsupportedConfig".
          defaultAudioCodec: 'mp4a.40.2',
          // The viewer often hits "Watch" before SRS has written the first
          // HLS segment, so the manifest 404s for a second or two. Without
          // these large retry budgets hls.js gives up after ~6 attempts and
          // the user has to manually reload the page once segments appear.
          manifestLoadingMaxRetry: 20,
          manifestLoadingRetryDelay: 500,
          manifestLoadingMaxRetryTimeout: 8000,
          levelLoadingMaxRetry: 20,
          levelLoadingRetryDelay: 500,
          levelLoadingMaxRetryTimeout: 8000,
          fragLoadingMaxRetry: 10,
          fragLoadingRetryDelay: 500,
          fragLoadingMaxRetryTimeout: 8000,
        });
        hls.loadSource(playbackUrl);
        hls.attachMedia(video);

        // Manual fallback retry loop for when the manifest is still missing
        // after hls.js exhausts its built-in retries (cold-start race vs SRS).
        const scheduleManifestRetry = () => {
          if (manifestRetryTimer) return;
          manifestRetryTimer = window.setTimeout(() => {
            manifestRetryTimer = null;
            try {
              hls.loadSource(playbackUrl);
              hls.startLoad();
            } catch {
              /* destroyed */
            }
          }, 1000);
        };

        hls.on(Hls.Events.ERROR, (_event, data) => {
          // Manifest 404 while SRS is still warming up — keep retrying
          // instead of surfacing a fatal error to the user.
          if (
            data.details === Hls.ErrorDetails.MANIFEST_LOAD_ERROR ||
            data.details === Hls.ErrorDetails.MANIFEST_LOAD_TIMEOUT ||
            data.details === Hls.ErrorDetails.LEVEL_LOAD_ERROR ||
            data.details === Hls.ErrorDetails.LEVEL_LOAD_TIMEOUT
          ) {
            setPlayerError('Waiting for the broadcaster to come online...');
            scheduleManifestRetry();
            return;
          }

          if (!data.fatal) return;
          switch (data.type) {
            case Hls.ErrorTypes.NETWORK_ERROR:
              scheduleManifestRetry();
              return;
            case Hls.ErrorTypes.MEDIA_ERROR:
              hls.recoverMediaError();
              return;
            default:
              setPlayerError('Stream is not available yet. Waiting for the broadcaster...');
          }
        });

        hls.on(Hls.Events.MANIFEST_PARSED, () => {
          if (manifestRetryTimer) {
            window.clearTimeout(manifestRetryTimer);
            manifestRetryTimer = null;
          }
          updatePlaybackMetrics({
            manifestLoadMs: Math.round(performance.now() - playbackStartedAt),
          });
          setPlayerError('');
          video.play().catch(() => {});
        });

        hls.on(Hls.Events.LEVEL_UPDATED, () => updateBufferMetrics(hls));

        hlsRef.current = hls;
        metricTimer = window.setInterval(() => updateBufferMetrics(hls), 1000);
      } else if (videoRef.current.canPlayType('application/vnd.apple.mpegurl')) {
        // Safari native HLS.
        const handleLoadedMetadata = () => {
          updatePlaybackMetrics({
            manifestLoadMs: Math.round(performance.now() - playbackStartedAt),
          });
          video.play().catch(() => {});
        };

        video.addEventListener('loadedmetadata', handleLoadedMetadata, { once: true });
        video.src = playbackUrl;
        metricTimer = window.setInterval(() => updateBufferMetrics(null), 1000);
      } else {
        setPlayerError('Your browser does not support HLS playback.');
      }
    };

    // Try WHEP first if we have a stream key and the browser supports
    // RTCPeerConnection. Falls back to HLS on any failure. We send the
    // rendition-suffixed key so SRS serves the WebRTC bridge for the
    // requested quality, not always the source.
    if (playbackKey && typeof RTCPeerConnection === 'function') {
      const controller = new AbortController();
      whepAbort = controller;
      startWhepPlayback({ streamKey: playbackKey, video, signal: controller.signal })
        .then((cleanup) => {
          if (cancelled) {
            cleanup();
            return;
          }
          whepCleanup = cleanup;
          updatePlaybackMetrics({
            source: 'webrtc',
            manifestLoadMs: Math.round(performance.now() - playbackStartedAt),
          });
          setPlayerError('');
          metricTimer = window.setInterval(() => updateBufferMetrics(null), 1000);
        })
        .catch(() => {
          if (cancelled) return;
          // Reset video element so HLS can take over cleanly.
          try { video.srcObject = null; } catch { /* ignore */ }
          startHls();
        });
    } else {
      startHls();
    }

    return () => {
      cancelled = true;
      if (whepAbort) whepAbort.abort();
      if (whepCleanup) whepCleanup();
      if (metricTimer) {
        window.clearInterval(metricTimer);
      }
      if (manifestRetryTimer) {
        window.clearTimeout(manifestRetryTimer);
        manifestRetryTimer = null;
      }
      video.removeEventListener('playing', handlePlaying);
      if (hlsRef.current) {
        hlsRef.current.destroy();
        hlsRef.current = null;
      }
    };
  }, [playbackUrl, playbackKey, updatePlaybackMetrics]);

  const metricLabel = (value) => (value == null ? '...' : `${value} ms`);

  return (
    <div className="page">
      <div className="watch-layout">
        <div className="watch-main">
          {error && <p className="error-msg">{error}</p>}

          {!error && (
            <>
              <div className="video-wrapper">
                <video ref={videoRef} controls />
                {stream?.requires_password && !hlsUrl && !error && (
                  <div className="video-placeholder" style={{ padding: '1rem' }}>
                    <p style={{ marginBottom: '0.5rem' }}>This stream is password protected.</p>
                    <form
                      onSubmit={(e) => {
                        e.preventDefault();
                        unlockStream(joinPassword);
                      }}
                      style={{ display: 'flex', gap: '0.5rem', justifyContent: 'center' }}
                    >
                      <input
                        type="password"
                        value={joinPassword}
                        onChange={(e) => setJoinPassword(e.target.value)}
                        placeholder="Enter stream password"
                        style={{ padding: '0.5rem', minWidth: '220px' }}
                      />
                      <button className="btn" type="submit" disabled={joining}>
                        {joining ? 'Joining...' : 'Join Stream'}
                      </button>
                    </form>
                    {joinError && <p className="error-msg mt-1">{joinError}</p>}
                  </div>
                )}
                {playerError && (
                  <div className="video-placeholder">{playerError}</div>
                )}
              </div>

              <div className="mt-2">
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '1rem', flexWrap: 'wrap' }}>
                  <h2 style={{ margin: 0 }}>
                    {stream?.is_live && <span className="live-badge">LIVE</span>}{' '}
                    {stream?.title || 'Loading...'}
                  </h2>
                  {/* Manual quality selection. Changing this restarts the
                      player against the rendition's HLS playlist / WHEP
                      endpoint — there's no auto/ABR, the user picks. */}
                  {hlsUrl && (
                    <label style={{ display: 'flex', alignItems: 'center', gap: '0.4rem', fontSize: '0.85rem' }}>
                      Quality:
                      <select
                        value={quality}
                        onChange={(e) => setQuality(e.target.value)}
                        style={{ padding: '0.25rem 0.5rem' }}
                      >
                        {QUALITY_OPTIONS.map((opt) => (
                          <option key={opt.value} value={opt.value}>{opt.label}</option>
                        ))}
                      </select>
                    </label>
                  )}
                </div>
                <p style={{ color: 'var(--color-text-muted)', fontSize: '0.9rem' }}>
                  {stream?.username || (stream ? `User #${stream.user_id}` : '')}
                </p>
                <div style={{ color: 'var(--color-text-muted)', fontSize: '0.85rem' }}>
                  <p>
                    Source: {playbackMetrics.source || 'pending'}
                    {playbackMetrics.edgeId ? ` (${playbackMetrics.edgeId})` : ''}
                  </p>
                  <p>Route lookup: {metricLabel(playbackMetrics.routeLookupMs)}</p>
                  <p>Manifest load: {metricLabel(playbackMetrics.manifestLoadMs)}</p>
                  <p>First frame: {metricLabel(playbackMetrics.firstFrameMs)}</p>
                  <p>Estimated live latency: {metricLabel(playbackMetrics.liveLatencyMs)}</p>
                  <p>Buffered ahead: {metricLabel(playbackMetrics.bufferMs)}</p>
                </div>
              </div>
            </>
          )}
        </div>

        {!error && (
          <aside className="watch-chat-column">
            <StreamChat streamId={id} chatToken={chatToken} />
          </aside>
        )}
      </div>
    </div>
  );
}

export default Watch;
