import React, { useEffect, useRef, useState } from 'react';
import { useParams } from 'react-router-dom';
import Hls from 'hls.js';
import { getStream } from '../services/api';

/**
 * Watch page — plays an HLS stream.
 *
 * In Sprint 1 the media pipeline isn't wired up yet, so the player will
 * show a placeholder when there's no HLS source available. Once Sprint 2
 * connects RTMP → HLS this page will "just work".
 */
function Watch() {
  const { id } = useParams();
  const videoRef = useRef(null);
  const hlsRef = useRef(null);

  const [stream, setStream] = useState(null);
  const [error, setError] = useState('');
  const [playerError, setPlayerError] = useState('');

  // Fetch stream details from the backend.
  useEffect(() => {
    let cancelled = false;

    const fetchStream = async () => {
      try {
        const res = await getStream(id);
        if (!cancelled) setStream(res.data);
      } catch {
        if (!cancelled) setError('Stream not found.');
      }
    };

    fetchStream();
    return () => { cancelled = true; };
  }, [id]);

  // Attach HLS.js when a stream_key becomes available.
  useEffect(() => {
    if (!stream?.stream_key || !videoRef.current) return;

    const hlsUrl = `/hls/${stream.stream_key}/index.m3u8`;

    if (Hls.isSupported()) {
      const hls = new Hls({ enableWorker: true, lowLatencyMode: true });
      hls.loadSource(hlsUrl);
      hls.attachMedia(videoRef.current);

      hls.on(Hls.Events.ERROR, (_event, data) => {
        if (data.fatal) {
          setPlayerError('Stream is not available yet. Waiting for the broadcaster…');
        }
      });

      hls.on(Hls.Events.MANIFEST_PARSED, () => {
        setPlayerError('');
        videoRef.current.play().catch(() => {});
      });

      hlsRef.current = hls;
    } else if (videoRef.current.canPlayType('application/vnd.apple.mpegurl')) {
      // Safari native HLS
      videoRef.current.src = hlsUrl;
    } else {
      setPlayerError('Your browser does not support HLS playback.');
    }

    return () => {
      if (hlsRef.current) {
        hlsRef.current.destroy();
        hlsRef.current = null;
      }
    };
  }, [stream]);

  return (
    <div className="page">
      <div className="watch-container">
        {error && <p className="error-msg">{error}</p>}

        {!error && (
          <>
            <div className="video-wrapper">
              <video ref={videoRef} controls />
              {playerError && (
                <div className="video-placeholder">{playerError}</div>
              )}
            </div>

            <div className="mt-2">
              <h2>
                {stream?.is_live && <span className="live-badge">LIVE</span>}{' '}
                {stream?.title || 'Loading…'}
              </h2>
              <p style={{ color: 'var(--color-text-muted)', fontSize: '0.9rem' }}>
                {stream?.username || (stream ? `User #${stream.user_id}` : '')}
              </p>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

export default Watch;
