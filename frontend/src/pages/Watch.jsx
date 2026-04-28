import React, { useEffect, useRef, useState } from 'react';
import { useParams } from 'react-router-dom';
import Hls from 'hls.js';
import { getStream, joinStream } from '../services/api';

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

  const unlockStream = async (password = '') => {
    try {
      setJoining(true);
      setJoinError('');
      const res = await joinStream(id, password);
      setHlsUrl(res.data?.hls_url || '');
    } catch (err) {
      const msg = err.response?.data?.error || 'Unable to join this stream.';
      setJoinError(msg);
      setHlsUrl('');
    } finally {
      setJoining(false);
    }
  };

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
  }, [id]);

  // Attach HLS.js when a stream URL is available.
  useEffect(() => {
    if (!hlsUrl || !videoRef.current) return;

    if (Hls.isSupported()) {
      const hls = new Hls({ enableWorker: true, lowLatencyMode: true });
      hls.loadSource(hlsUrl);
      hls.attachMedia(videoRef.current);

      hls.on(Hls.Events.ERROR, (_event, data) => {
        if (data.fatal) {
          setPlayerError('Stream is not available yet. Waiting for the broadcaster...');
        }
      });

      hls.on(Hls.Events.MANIFEST_PARSED, () => {
        setPlayerError('');
        videoRef.current.play().catch(() => {});
      });

      hlsRef.current = hls;
    } else if (videoRef.current.canPlayType('application/vnd.apple.mpegurl')) {
      // Safari native HLS.
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
  }, [hlsUrl]);

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
            </div>
          </>
        )}
      </div>
    </div>
  );
}

export default Watch;
