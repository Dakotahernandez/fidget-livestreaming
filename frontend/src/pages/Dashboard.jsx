import React, { useEffect, useRef, useState } from 'react';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';
import useAuth from '../hooks/useAuth';
import {
  createStream,
  deleteStream,
  deleteSound,
  getConnectStatus,
  getDonationHistory,
  listMyStreams,
  listSounds,
  listStreams,
  startOnboarding,
  uploadSound,
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
  const [searchParams, setSearchParams] = useSearchParams();

  const [myStreams, setMyStreams] = useState([]);
  const [liveStreams, setLiveStreams] = useState([]);
  const [title, setTitle] = useState('');
  const [accessPassword, setAccessPassword] = useState('');
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [showKey, setShowKey] = useState({});

  // Stripe Connect onboarding state.
  const [connect, setConnect] = useState({ onboarded: false, account_id: null, loaded: false });
  const [connectError, setConnectError] = useState('');
  const [connectBusy, setConnectBusy] = useState(false);
  const [connectToast, setConnectToast] = useState('');

  // Donation sound library state.
  const [sounds, setSounds] = useState([]);
  const [soundError, setSoundError] = useState('');
  const [soundDisplayName, setSoundDisplayName] = useState('');
  const soundFileInputRef = useRef(null);
  const [uploadingSound, setUploadingSound] = useState(false);
  const SOUNDS_MAX = 10;

  // Donation history state.
  const [donationHistory, setDonationHistory] = useState([]);

  // Pick the first owned stream as the "active" id for sound listing.
  // Sound library is per-streamer, but `listSounds` is keyed by stream
  // for unauthenticated viewers; reuse it here so the streamer sees the
  // same list a donor would.
  const activeStream = myStreams.find((s) => s.is_live) || myStreams[0];
  const activeStreamId = activeStream?.id;

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

  // ── Stripe Connect ────────────────────────────────────────────────
  // Initial fetch on mount + auto-refresh after a successful onboarding
  // redirect (`?connect=success`). The backend re-queries Stripe so the
  // local `stripe_onboarding_complete` flag flips as soon as
  // `charges_enabled` does.
  const refreshConnectStatus = React.useCallback(async () => {
    if (!isAuthenticated) return;
    try {
      const res = await getConnectStatus();
      setConnect({ ...res.data, loaded: true });
    } catch {
      setConnect((prev) => ({ ...prev, loaded: true }));
    }
  }, [isAuthenticated]);

  useEffect(() => {
    refreshConnectStatus();
  }, [refreshConnectStatus]);

  useEffect(() => {
    if (searchParams.get('connect') === 'success') {
      setConnectToast('Payments enabled! Welcome to monetization.');
      refreshConnectStatus();
      // Clean the URL so a refresh doesn't re-show the toast.
      const next = new URLSearchParams(searchParams);
      next.delete('connect');
      setSearchParams(next, { replace: true });
    }
  }, [searchParams, setSearchParams, refreshConnectStatus]);

  const handleStartOnboarding = async () => {
    setConnectError('');
    setConnectBusy(true);
    try {
      const res = await startOnboarding();
      const url = res.data?.url;
      if (!url) throw new Error('no onboarding url');
      window.location.assign(url);
    } catch (err) {
      setConnectError(err.response?.data?.error || 'Could not start onboarding.');
      setConnectBusy(false);
    }
  };

  // ── Donation sound library ────────────────────────────────────────
  const refreshSounds = React.useCallback(async () => {
    if (!activeStreamId || !connect.onboarded) return;
    try {
      const res = await listSounds(activeStreamId);
      setSounds(res.data || []);
    } catch {
      setSounds([]);
    }
  }, [activeStreamId, connect.onboarded]);

  useEffect(() => {
    refreshSounds();
  }, [refreshSounds]);

  const handleUploadSound = async (e) => {
    e.preventDefault();
    setSoundError('');
    const file = soundFileInputRef.current?.files?.[0];
    const name = soundDisplayName.trim();
    if (!name) {
      setSoundError('Display name is required.');
      return;
    }
    if (!file) {
      setSoundError('Pick an audio file to upload.');
      return;
    }
    const formData = new FormData();
    formData.append('display_name', name);
    formData.append('file', file);
    setUploadingSound(true);
    try {
      await uploadSound(formData);
      setSoundDisplayName('');
      if (soundFileInputRef.current) soundFileInputRef.current.value = '';
      await refreshSounds();
    } catch (err) {
      setSoundError(err.response?.data?.error || 'Upload failed.');
    } finally {
      setUploadingSound(false);
    }
  };

  const handleDeleteSound = async (id) => {
    setSoundError('');
    try {
      await deleteSound(id);
      await refreshSounds();
    } catch (err) {
      setSoundError(err.response?.data?.error || 'Delete failed.');
    }
  };

  // ── Donation history ──────────────────────────────────────────────
  useEffect(() => {
    if (!user?.id || !connect.onboarded) return;
    let cancelled = false;
    (async () => {
      try {
        const res = await getDonationHistory(user.id);
        if (!cancelled) setDonationHistory(res.data || []);
      } catch {
        if (!cancelled) setDonationHistory([]);
      }
    })();
    return () => { cancelled = true; };
  }, [user?.id, connect.onboarded]);

  if (!isAuthenticated) return null;

  return (
    <div className="page">
      <h2>Dashboard</h2>

      {error && <p className="error-msg mt-1">{error}</p>}

      {/* ── Monetization ─────────────────────────────────────────── */}
      <div className="dashboard-money-section mt-2">
        <h3>Monetization</h3>
        {connectToast && (
          <p className="donation-banner mt-1">{connectToast}</p>
        )}
        {!connect.loaded && (
          <p style={{ color: 'var(--color-text-muted)' }}>Loading payout status…</p>
        )}
        {connect.loaded && connect.onboarded && (
          <p className="connect-status-ok">● Payments enabled</p>
        )}
        {connect.loaded && !connect.onboarded && (
          <>
            <p style={{ color: 'var(--color-text-muted)', fontSize: '0.9rem' }}>
              Connect a Stripe account to start accepting donations. Streamers get 85% of each
              donation; the platform keeps 15% (test mode only).
            </p>
            <button
              type="button"
              className="btn"
              disabled={connectBusy}
              onClick={handleStartOnboarding}
              style={{ marginTop: '0.5rem' }}
            >
              {connectBusy ? 'Redirecting…' : 'Connect Stripe Account'}
            </button>
          </>
        )}
        {connectError && <p className="error-msg mt-1">{connectError}</p>}
      </div>

      {/* ── Donation sounds ──────────────────────────────────────── */}
      {connect.loaded && connect.onboarded && (
        <div className="dashboard-money-section mt-2">
          <h3>Donation Sounds</h3>
          <p style={{ color: 'var(--color-text-muted)', fontSize: '0.85rem' }}>
            {sounds.length} / {SOUNDS_MAX} sounds used. 2 MB max per file.
          </p>
          {sounds.length === 0 && (
            <p style={{ color: 'var(--color-text-muted)' }}>No sounds uploaded yet.</p>
          )}
          {sounds.length > 0 && (
            <ul style={{ listStyle: 'none', padding: 0, marginTop: '0.5rem' }}>
              {sounds.map((s) => (
                <li
                  key={s.id}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    padding: '0.4rem 0',
                    borderBottom: '1px solid var(--color-border-subtle)',
                  }}
                >
                  <span>{s.display_name}</span>
                  <button
                    type="button"
                    className="btn btn--outline"
                    onClick={() => handleDeleteSound(s.id)}
                  >
                    Delete
                  </button>
                </li>
              ))}
            </ul>
          )}
          <form onSubmit={handleUploadSound} style={{ marginTop: '0.75rem', display: 'flex', gap: '0.5rem', flexWrap: 'wrap' }}>
            <input
              type="text"
              placeholder="Display name"
              value={soundDisplayName}
              onChange={(e) => setSoundDisplayName(e.target.value)}
              maxLength={50}
              style={{ flex: 1, minWidth: '160px', padding: '0.5rem' }}
            />
            <input
              type="file"
              accept="audio/*"
              ref={soundFileInputRef}
              style={{ flex: 2, minWidth: '200px' }}
            />
            <button
              type="submit"
              className="btn"
              disabled={uploadingSound || sounds.length >= SOUNDS_MAX || !activeStreamId}
            >
              {uploadingSound ? 'Uploading…' : 'Upload'}
            </button>
          </form>
          {!activeStreamId && (
            <p className="mt-1" style={{ color: 'var(--color-text-muted)', fontSize: '0.85rem' }}>
              Create a stream first so the donor sound picker has something to load.
            </p>
          )}
          {soundError && <p className="error-msg mt-1">{soundError}</p>}
        </div>
      )}

      {/* ── Donation history ─────────────────────────────────────── */}
      {connect.loaded && connect.onboarded && (
        <div className="dashboard-money-section mt-2">
          <h3>Recent Donations</h3>
          {donationHistory.length === 0 ? (
            <p style={{ color: 'var(--color-text-muted)' }}>No donations yet.</p>
          ) : (
            <table style={{ width: '100%', marginTop: '0.5rem', borderCollapse: 'collapse' }}>
              <thead>
                <tr style={{ textAlign: 'left', color: 'var(--color-text-muted)', fontSize: '0.85rem' }}>
                  <th style={{ padding: '0.4rem 0' }}>When</th>
                  <th>Donor</th>
                  <th>Stream</th>
                  <th style={{ textAlign: 'right' }}>Amount</th>
                </tr>
              </thead>
              <tbody>
                {donationHistory.map((d) => (
                  <tr key={d.id} style={{ borderTop: '1px solid var(--color-border-subtle)' }}>
                    <td style={{ padding: '0.4rem 0', fontSize: '0.85rem' }}>
                      {new Date(d.created_at).toLocaleString()}
                    </td>
                    <td>{d.donor_name}</td>
                    <td style={{ color: 'var(--color-text-muted)' }}>{d.stream_title}</td>
                    <td style={{ textAlign: 'right', fontWeight: 600 }}>
                      ${(d.amount_cents / 100).toFixed(2)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}

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
