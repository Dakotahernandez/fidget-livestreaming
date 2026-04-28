import React, { useEffect, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import useAuth from '../hooks/useAuth';
import { createStream, listMyStreams, listStreams } from '../services/api';
import ScreenCapture from '../components/streamer/ScreenCapture';

/** RTMP ingest URL — points to the media-server container in dev. */
const RTMP_URL = 'rtmp://localhost:1935/live';

function Dashboard() {
  const { isAuthenticated } = useAuth();
  const navigate = useNavigate();

  const [myStreams, setMyStreams] = useState([]);
  const [liveStreams, setLiveStreams] = useState([]);
  const [title, setTitle] = useState('');
  const [accessPassword, setAccessPassword] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [showKey, setShowKey] = useState({});

  // Redirect to login if not authenticated.
  useEffect(() => {
    if (!isAuthenticated) {
      navigate('/login');
    }
  }, [isAuthenticated, navigate]);

  // Fetch personal streams and globally live streams on an interval.
  useEffect(() => {
    if (!isAuthenticated) return;

    let cancelled = false;

    const fetchStreams = async () => {
      const [mineResult, liveResult] = await Promise.allSettled([
        listMyStreams(),
        listStreams(),
      ]);

      if (cancelled) return;

      let nextError = '';

      if (mineResult.status === 'fulfilled') {
        setMyStreams(mineResult.value.data || []);
      } else {
        nextError = 'Failed to load your streams.';
      }

      if (liveResult.status === 'fulfilled') {
        setLiveStreams(liveResult.value.data || []);
      } else {
        nextError = nextError
          ? `${nextError} Failed to load live streams.`
          : 'Failed to load live streams.';
      }

      setError(nextError);
      setLoading(false);
    };

    fetchStreams();
    const poll = setInterval(fetchStreams, 5000);

    return () => {
      cancelled = true;
      clearInterval(poll);
    };
  }, [isAuthenticated]);

  const handleCreate = async (e) => {
    e.preventDefault();
    setError('');

    try {
      const res = await createStream(title || 'My Stream', accessPassword);
      setMyStreams((prev) => [...prev, res.data]);
      setTitle('');
      setAccessPassword('');
    } catch (err) {
      setError(err.response?.data?.message || 'Could not create stream.');
    }
  };

  const toggleShowKey = (id) =>
    setShowKey((prev) => ({ ...prev, [id]: !prev[id] }));

  if (!isAuthenticated) return null;

  return (
    <div className="page">
      <h2>Dashboard</h2>

      {error && <p className="error-msg mt-1">{error}</p>}

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
          <input
            type="password"
            placeholder="Optional stream password"
            value={accessPassword}
            onChange={(e) => setAccessPassword(e.target.value)}
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
        <p className="mt-1" style={{ color: 'var(--color-text-muted)', fontSize: '0.85rem' }}>
          Leave password blank to create a public stream.
        </p>
      </div>

      <div className="dashboard-section mt-2">
        <h3>Live Now (All Users)</h3>
        {liveStreams.length > 0 ? (
          <div className="stream-grid" style={{ marginTop: '0.75rem' }}>
            {liveStreams.map((stream) => (
              <Link
                to={`/watch/${stream.id}`}
                key={stream.id}
                style={{ textDecoration: 'none', color: 'inherit' }}
              >
                <div className="stream-card">
                  <div className="stream-card__title">
                    <span className="live-badge">LIVE</span>{' '}
                    {stream.title || 'Untitled Stream'}
                  </div>
                  <div className="stream-card__meta">
                    {stream.requires_password ? '[LOCKED] ' : ''}
                    {stream.username || `User #${stream.user_id}`}
                  </div>
                </div>
              </Link>
            ))}
          </div>
        ) : (
          <p className="mt-1" style={{ color: 'var(--color-text-muted)' }}>
            No active streams right now.
          </p>
        )}
      </div>

      {loading && <p className="text-center mt-2">Loading…</p>}

      {myStreams.map((stream) => (
        <div className="dashboard-section" key={stream.id}>
          <h3>
            {stream.title || 'Untitled Stream'}{' '}
            {stream.is_live && <span className="live-badge">LIVE</span>}
          </h3>

          <p style={{ fontSize: '0.85rem', color: 'var(--color-text-muted)' }}>
            Visibility: {stream.requires_password ? 'Password protected' : 'Public'}
          </p>

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

          <p style={{ fontSize: '0.85rem', color: 'var(--color-text-muted)', marginTop: '0.75rem' }}>
            Start and stop your live broadcast from the Screen Capture controls below.
          </p>

          <ScreenCapture streamId={stream.id} />
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
