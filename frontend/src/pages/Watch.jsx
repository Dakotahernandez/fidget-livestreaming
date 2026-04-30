import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useParams } from 'react-router-dom';
import Hls from 'hls.js';
import { getStream, joinStream, resolvePlaybackRoute } from '../services/api';

const EMPTY_PLAYBACK_METRICS = {
  source: '',
  edgeId: '',
  routeLookupMs: null,
  manifestLoadMs: null,
  firstFrameMs: null,
  liveLatencyMs: null,
  bufferMs: null,
};

/**
 * Watch page — plays an HLS stream.
 */
function Watch() {
  const { id } = useParams();
  const videoRef = useRef(null);
  const hlsRef = useRef(null);

  const [stream, setStream] = useState(null);
  const [hlsUrl, setHlsUrl] = useState('');
  const [error, setError] = useState('');
  const [playerError, setPlayerError] = useState('');
  const [joinPassword, setJoinPassword] = useState('');
  const [joinError, setJoinError] = useState('');
  const [joining, setJoining] = useState(false);
  const [playbackMetrics, setPlaybackMetrics] = useState(EMPTY_PLAYBACK_METRICS);

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
      const streamKey = res.data?.stream_key || '';

      let nextUrl = fallbackUrl;
      let playbackSource = 'origin';
      let edgeId = '';

      if (streamKey) {
        try {
          const routeRes = await resolvePlaybackRoute(streamKey);
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
      setHlsUrl(nextUrl);
    } catch (err) {
      const msg = err.response?.data?.error || 'Unable to join this stream.';
      setJoinError(msg);
      setHlsUrl('');
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

  // Attach HLS.js when a stream URL is available.
  useEffect(() => {
    if (!hlsUrl || !videoRef.current) return;

    const video = videoRef.current;
    const playbackStartedAt = performance.now();
    let metricTimer = null;
    let manifestRetryTimer = null;

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
      hls.loadSource(hlsUrl);
      hls.attachMedia(video);

      // Manual fallback retry loop for when the manifest is still missing
      // after hls.js exhausts its built-in retries (cold-start race vs SRS).
      const scheduleManifestRetry = () => {
        if (manifestRetryTimer) return;
        manifestRetryTimer = window.setTimeout(() => {
          manifestRetryTimer = null;
          try {
            hls.loadSource(hlsUrl);
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
      video.src = hlsUrl;
      metricTimer = window.setInterval(() => updateBufferMetrics(null), 1000);
    } else {
      setPlayerError('Your browser does not support HLS playback.');
    }

    return () => {
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
  }, [hlsUrl, updatePlaybackMetrics]);

  const metricLabel = (value) => (value == null ? '...' : `${value} ms`);

  return (
    <div className="page">
      <div className="watch-container">
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
              <h2>
                {stream?.is_live && <span className="live-badge">LIVE</span>}{' '}
                {stream?.title || 'Loading...'}
              </h2>
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
    </div>
  );
}

export default Watch;
