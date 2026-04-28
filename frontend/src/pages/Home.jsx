import React, { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { listStreams } from '../services/api';

function Home() {
  const [streams, setStreams] = useState([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  useEffect(() => {
    let cancelled = false;

    const fetchStreams = async () => {
      try {
        const res = await listStreams();
        if (!cancelled) {
          setStreams(res.data);
          setError('');
        }
      } catch (err) {
        if (!cancelled) {
          setError('Could not load streams. Is the backend running?');
        }
      } finally {
        if (!cancelled) {
          setLoading(false);
        }
      }
    };

    fetchStreams();

    const poll = setInterval(fetchStreams, 5000);

    return () => {
      cancelled = true;
      clearInterval(poll);
    };
  }, []);

  return (
    <div className="page">
      <h2>Live Streams</h2>

      {loading && <p className="mt-2 text-center">Loading…</p>}
      {error && <p className="error-msg mt-2">{error}</p>}

      {!loading && !error && streams.length === 0 && (
        <p className="mt-2 text-center" style={{ color: 'var(--color-text-muted)' }}>
          No one is live right now.
        </p>
      )}

      <div className="stream-grid">
        {streams.map((stream) => (
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
    </div>
  );
}

export default Home;
