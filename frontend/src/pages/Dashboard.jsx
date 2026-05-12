import React, { useEffect, useRef, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import useAuth from '../hooks/useAuth';
import {
  createStream,
  deleteStream,
  listMyRoleApplications,
  listMyStreams,
  listStreams,
  submitRoleApplication,
} from '../services/api';
import ScreenCapture from '../components/streamer/ScreenCapture';

/** RTMP ingest URL — points to the media-server container in dev. */
const RTMP_URL = 'rtmp://localhost:1935/live';

/**
 * How long a stream must remain `is_live=false` before the dashboard
 * auto-deletes it. Anything shorter risks pruning a stream the user is
 * actively broadcasting when the backend's `is_live` flag flickers
 * (rendition publish hiccup, on_publish_done arriving briefly between
 * source restarts, etc.). 60s is comfortably longer than any normal
 * publish gap and short enough that abandoned stream rows still get
 * cleaned up promptly.
 */
const STALE_OFFLINE_GRACE_MS = 60_000;

function Dashboard() {
  const { isAuthenticated, user } = useAuth();
  const navigate = useNavigate();

  const [myStreams, setMyStreams] = useState([]);
  const [liveStreams, setLiveStreams] = useState([]);
  const [title, setTitle] = useState('');
  const [accessPassword, setAccessPassword] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [showKey, setShowKey] = useState({});
  const [applicationReason, setApplicationReason] = useState('');
  const [requestedRole, setRequestedRole] = useState('streamer');
  const [myRoleApplications, setMyRoleApplications] = useState([]);
  const [applicationError, setApplicationError] = useState('');
  const [applying, setApplying] = useState(false);

  // Stream ids the user just created in this browser session. We keep them
  // visible (so the broadcast panel stays mounted before Go Live) even
  // though they aren't `is_live` yet.
  const recentlyCreatedRef = useRef(new Set());
  const pruningRef = useRef(new Set());

  // Per-stream timestamp of the first poll where we observed the stream
  // as not-live. Used to give a grace period before auto-pruning so a
  // transient `is_live=false` blip (e.g., a brief rendition publish drop,
  // or the gap between create and Go Live after a page reload) doesn't
  // tear down a stream the user is actively broadcasting.
  const firstOfflineAtRef = useRef(new Map());

  const canStream = user?.role === 'admin' || user?.role === 'streamer';
  const latestRoleApplication = myRoleApplications[0];
  const hasPendingRoleApplication = latestRoleApplication?.status === 'pending';

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
        const allMine = mineResult.value.data || [];
        const now = Date.now();
        const presentIds = new Set(allMine.map((s) => s.id));

        // Update the per-stream first-offline timestamps. A stream that
        // is currently live clears its timestamp; a not-live stream
        // records `now` if we haven't seen it offline before. Streams
        // the backend has stopped returning at all are dropped from the
        // map so it doesn't grow forever.
        for (const id of Array.from(firstOfflineAtRef.current.keys())) {
          if (!presentIds.has(id)) firstOfflineAtRef.current.delete(id);
        }
        for (const s of allMine) {
          if (s.is_live) {
            firstOfflineAtRef.current.delete(s.id);
          } else if (!firstOfflineAtRef.current.has(s.id)) {
            firstOfflineAtRef.current.set(s.id, now);
          }
        }

        // Auto-prune ephemeral streams: not live, not recently created
        // in this session, AND have been continuously offline for at
        // least the grace period. The grace period is what protects the
        // panel from disappearing on a transient is_live=false blip.
        const stale = allMine.filter((s) => {
          if (s.is_live) return false;
          if (recentlyCreatedRef.current.has(s.id)) return false;
          const firstOffline = firstOfflineAtRef.current.get(s.id);
          return (
            firstOffline !== undefined &&
            now - firstOffline > STALE_OFFLINE_GRACE_MS
          );
        });
        for (const s of stale) {
          if (pruningRef.current.has(s.id)) continue;
          pruningRef.current.add(s.id);
          deleteStream(s.id)
            .catch(() => { /* best-effort cleanup */ })
            .finally(() => pruningRef.current.delete(s.id));
        }

        // Visibility filter mirrors the prune logic: a stream stays
        // mounted if it's live, was just created, or is still inside
        // its offline grace window. This avoids the panel flickering
        // out one poll before deleteStream completes.
        const visible = allMine.filter((s) => {
          if (s.is_live) return true;
          if (recentlyCreatedRef.current.has(s.id)) return true;
          const firstOffline = firstOfflineAtRef.current.get(s.id);
          return (
            firstOffline === undefined ||
            now - firstOffline <= STALE_OFFLINE_GRACE_MS
          );
        });
        setMyStreams(visible);
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

  useEffect(() => {
    if (!isAuthenticated || user?.role !== 'viewer') {
      setMyRoleApplications([]);
      return;
    }

    let cancelled = false;

    const fetchMyApplications = async () => {
      try {
        const res = await listMyRoleApplications();
        if (!cancelled) {
          setMyRoleApplications(res.data || []);
        }
      } catch (err) {
        if (!cancelled) {
          setApplicationError(
            err.response?.data?.error ||
            err.response?.data?.message ||
            'Could not load role applications.',
          );
        }
      }
    };

    fetchMyApplications();

    return () => {
      cancelled = true;
    };
  }, [isAuthenticated, user?.role]);

  const handleCreate = async (e) => {
    e.preventDefault();
    setError('');

    try {
      const res = await createStream(title || 'My Stream', accessPassword);
      recentlyCreatedRef.current.add(res.data.id);
      setMyStreams((prev) => [...prev, res.data]);
      setTitle('');
      setAccessPassword('');
    } catch (err) {
      setError(err.response?.data?.message || 'Could not create stream.');
    }
  };

  const toggleShowKey = (id) =>
    setShowKey((prev) => ({ ...prev, [id]: !prev[id] }));

  const handleApplyForStreamer = async (e) => {
    e.preventDefault();
    setApplicationError('');
    setApplying(true);

    try {
      const res = await submitRoleApplication(applicationReason, requestedRole);
      setMyRoleApplications((current) => [res.data, ...current]);
      setApplicationReason('');
    } catch (err) {
      setApplicationError(
        err.response?.data?.error ||
        err.response?.data?.message ||
        'Could not submit role application.',
      );
    } finally {
      setApplying(false);
    }
  };

  if (!isAuthenticated) return null;

  return (
    <div className="page">
      <h2>Dashboard</h2>

      {error && <p className="error-msg mt-1">{error}</p>}


      {user?.role === 'viewer' && (
        <>
          <p className="mt-1" style={{ color: 'var(--color-text-muted)' }}>
            You are signed in as a viewer. You can watch live streams, but you cannot create streams.
          </p>

          <div className="dashboard-section mt-2">
            <h3>Apply to Become a Streamer</h3>

            <p className="mt-1" style={{ color: 'var(--color-text-muted)' }}>
              Submit a request and an admin can approve your requested role.
            </p>

            <form onSubmit={handleApplyForStreamer} style={{ marginTop: '0.75rem' }}>
              <div style={{ marginBottom: '0.5rem' }}>
                <label style={{ display: 'flex', alignItems: 'center', gap: '0.5rem' }}>
                  <span style={{ color: 'var(--color-text-muted)', fontSize: '0.85rem' }}>
                    Requested role:
                  </span>
                  <select
                    value={requestedRole}
                    onChange={(e) => setRequestedRole(e.target.value)}
                    disabled={applying || hasPendingRoleApplication}
                    style={{
                      padding: '0.35rem 0.5rem',
                      borderRadius: 'var(--radius)',
                      border: '1px solid var(--color-border)',
                      background: 'var(--color-bg)',
                      color: 'var(--color-text)',
                    }}
                  >
                    <option value="streamer">streamer</option>
                    <option value="admin">admin</option>
                  </select>
                </label>
              </div>

              <textarea
                value={applicationReason}
                onChange={(e) => setApplicationReason(e.target.value)}
                placeholder="Why should this role be approved?"
                rows={3}
                disabled={applying || hasPendingRoleApplication}
                style={{
                  width: '100%',
                  padding: '0.5rem 0.75rem',
                  background: 'var(--color-bg)',
                  border: '1px solid var(--color-border)',
                  borderRadius: 'var(--radius)',
                  color: 'var(--color-text)',
                }}
              />

              <div className="mt-1" style={{ display: 'flex', alignItems: 'center', gap: '0.75rem' }}>
                <button
                  className="btn"
                  type="submit"
                  disabled={applying || hasPendingRoleApplication}
                >
                  {applying ? 'Submitting...' : hasPendingRoleApplication ? 'Request Pending' : 'Submit Request'}
                </button>

                {latestRoleApplication && (
                  <span style={{ color: 'var(--color-text-muted)', fontSize: '0.85rem' }}>
                    Latest request status: {latestRoleApplication.status} ({latestRoleApplication.target_role})
                  </span>
                )}
              </div>
            </form>

            {applicationError && (
              <p className="error-msg mt-1">{applicationError}</p>
            )}
          </div>
        </>
      )}

      {canStream &&(
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
      )}

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

      {canStream && myStreams.map((stream) => (
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

          <ScreenCapture streamId={stream.id} streamKey={stream.stream_key} />
        </div>
      ))}

      {canStream && !loading && myStreams.length === 0 && (
        <p className="text-center mt-2" style={{ color: 'var(--color-text-muted)' }}>
          You don&apos;t have any streams yet. Create one above!
        </p>
      )}
    </div>
  );
}

export default Dashboard;
