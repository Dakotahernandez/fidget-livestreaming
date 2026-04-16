import React, { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import useAuth from '../hooks/useAuth';
import { createStream, listStreams, startStream, stopStream } from '../services/api';
import ScreenCapture from '../components/streamer/ScreenCapture';

/** RTMP ingest URL — points to the media-server container in dev. */
const RTMP_URL = 'rtmp://localhost:1935/live';

function Dashboard() {
  const { user, isAuthenticated } = useAuth();
  const navigate = useNavigate();

  const [myStreams, setMyStreams] = useState([]);
  const [title, setTitle] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [showKey, setShowKey] = useState({});

  // Redirect to login if not authenticated.
  useEffect(() => {
    if (!isAuthenticated) {
      navigate('/login');
    }
  }, [isAuthenticated, navigate]);

  // Fetch user's streams on mount.
  useEffect(() => {
    if (!isAuthenticated) return;

    const fetch = async () => {
      try {
        const res = await listStreams();
        // The API returns all live streams; filter to user's own.
        // Once the backend supports a /me/streams endpoint we can switch.
        const mine = (res.data || []).filter(
          (s) => s.user_id === user?.id,
        );
        setMyStreams(mine);
      } catch {
        setError('Failed to load your streams.');
      } finally {
        setLoading(false);
      }
    };

    fetch();
  }, [isAuthenticated, user]);

  const handleCreate = async (e) => {
    e.preventDefault();
    setError('');
    try {
      const res = await createStream(title || 'My Stream');
      setMyStreams((prev) => [...prev, res.data]);
      setTitle('');
    } catch (err) {
      setError(err.response?.data?.message || 'Could not create stream.');
    }
  };

  const toggleLive = async (stream) => {
    try {
      if (stream.is_live) {
        await stopStream(stream.id);
      } else {
        await startStream(stream.id);
      }
      setMyStreams((prev) =>
        prev.map((s) =>
          s.id === stream.id ? { ...s, is_live: !s.is_live } : s,
        ),
      );
    } catch (err) {
      setError(err.response?.data?.message || 'Could not update stream status.');
    }
  };

  const toggleShowKey = (id) =>
    setShowKey((prev) => ({ ...prev, [id]: !prev[id] }));

  if (!isAuthenticated) return null;

  return (
    <div className="page">
      <h2>Dashboard</h2>

      {error && <p className="error-msg mt-1">{error}</p>}

      {/* ── Create a stream ─────────────────────────────────────────── */}
      <div className="dashboard-section mt-2">
        <h3>Create a New Stream</h3>
        <form onSubmit={handleCreate} style={{ display: 'flex', gap: '0.5rem' }}>
          <input
            type="text"
            placeholder="Stream title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            style={{
              flex: 1,
              padding: '0.5rem 0.75rem',
              background: 'var(--color-bg)',
              border: '1px solid var(--color-border)',
              borderRadius: 'var(--radius)',
              color: 'var(--color-text)',
            }}
          />
          <button className="btn" type="submit">Create</button>
        </form>
      </div>

      {/* ── Create a stream ─────────────────────────────────────────── */}
      {loading && <p className="text-center mt-2">Loading…</p>}

      {myStreams.map((stream) => (
        <div className="dashboard-section" key={stream.id}>
          <h3>
            {stream.title || 'Untitled Stream'}{' '}
            {stream.is_live && <span className="live-badge">LIVE</span>}
          </h3>

          {/* Stream key */}
          <p style={{ fontSize: '0.85rem', color: 'var(--color-text-muted)' }}>
            Stream Key
          </p>
          <div className="key-display">
            <code>
              {showKey[stream.id] ? stream.stream_key : '••••••••••••••••'}
            </code>
            <button
              className="btn btn--outline"
              type="button"
              onClick={() => toggleShowKey(stream.id)}
              style={{ whiteSpace: 'nowrap' }}
            >
              {showKey[stream.id] ? 'Hide' : 'Show'}
            </button>
          </div>

          {/* OBS instructions */}
          <div className="mt-2" style={{ fontSize: '0.85rem', color: 'var(--color-text-muted)' }}>
            <p><strong>OBS Setup:</strong></p>
            <ol style={{ paddingLeft: '1.25rem', marginTop: '0.25rem' }}>
              <li>Open OBS &rarr; Settings &rarr; Stream</li>
              <li>Service: <strong>Custom…</strong></li>
              <li>Server: <code>{RTMP_URL}</code></li>
              <li>Stream Key: <em>(your stream key above)</em></li>
              <li>Click &quot;Start Streaming&quot;</li>
            </ol>
          </div>
            
            {/* Screen capture test — preview your screen before going live */}
          <ScreenCapture streamId={stream.id} />
          {/* Start / Stop toggle */}
          <div className="mt-2">
            <button
              className={`btn ${stream.is_live ? 'btn--danger' : ''}`}
              type="button"
              onClick={() => toggleLive(stream)}
            >
              {stream.is_live ? 'Stop Stream' : 'Go Live'}
            </button>
          </div>
        </div>
      ))}

      {!loading && myStreams.length === 0 && (
        <p className="text-center mt-2" style={{ color: 'var(--color-text-muted)' }}>
          You don&apos;t have any streams yet. Create one above!
        </p>
      )}
    </div>
  );
}

export default Dashboard;
