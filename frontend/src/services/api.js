import axios from 'axios';
import { clearStoredAuth, getStoredToken } from './authStorage';

const api = axios.create({
  baseURL: '/api',
});

const routerApi = axios.create({
  baseURL: '/router-api',
});

// ── JWT interceptor ──────────────────────────────────────────────────
// Attach the stored token to every outgoing request automatically.
api.interceptors.request.use((config) => {
  const token = getStoredToken();
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

// If a response comes back 401, clear the stale token so the UI can
// redirect to login.
api.interceptors.response.use(
  (response) => response,
  (error) => {
    if (error.response && error.response.status === 401) {
      clearStoredAuth();
    }
    return Promise.reject(error);
  },
);

// ── Auth endpoints ───────────────────────────────────────────────────
export const register = (username, password) =>
  api.post('/auth/register', { username, password });

export const login = (username, password) =>
  api.post('/auth/login', { username, password });

// ── Stream endpoints ─────────────────────────────────────────────────
export const createStream = (title, accessPassword = '') =>
  api.post('/streams', {
    title,
    access_password: accessPassword,
  });

export const listStreams = () =>
  api.get('/streams');

export const listMyStreams = () =>
  api.get('/streams/me');

export const getStream = (id) =>
  api.get(`/streams/${id}`);

export const joinStream = (id, password = '') =>
  api.post(`/streams/${id}/join`, { password });

export const resolvePlaybackRoute = (streamKey) =>
  routerApi.get(`/route/${streamKey}`);

export const startStream = (id) =>
  api.put(`/streams/${id}/start`);

export const stopStream = (id) =>
  api.put(`/streams/${id}/stop`);

export const deleteStream = (id) =>
  api.delete(`/streams/${id}`);

export const getCertHash = () =>
  api.get('/transport/cert-hash');

export const getTransportUrl = (streamId) => {
  const token = getStoredToken();
  const host = window.location.hostname;
  const quicPort = 4443;
  return `https://${host}:${quicPort}/ingest/${streamId}?token=${token}`;
};

// Same-origin WHIP (WebRTC-HTTP Ingestion Protocol) URL. Used by the
// streamer fallback path in browsers without WebTransport (Firefox,
// Safari). The frontend nginx proxies `/whip/` to SRS's
// `/rtc/v1/whip/` endpoint, which accepts an SDP offer and bridges
// the WebRTC publish into the existing RTMP -> HLS pipeline. The
// `app=live` matches SRS's `live` vhost; `stream` is the per-stream
// key the backend already issues for RTMP publish.
export const getWhipPublishUrl = (streamKey) => {
  if (!streamKey) return null;
  return `/whip/?app=live&stream=${encodeURIComponent(streamKey)}`;
};

// ── Admin endpoints ─────────────────────────────────────────────────
export const listUsers = () =>
  api.get('/admin/users');

export const updateUserRole = (id, role) =>
  api.patch(`/admin/users/${id}/role`, { role });

export const listPendingRoleApplications = () =>
  api.get('/admin/role-applications');

export const reviewRoleApplication = (id, action) =>
  api.patch(`/admin/role-applications/${id}`, { action });


// ── Role application endpoints ──────────────────────────────────────
export const submitRoleApplication = (reason = '', targetRole = 'streamer') =>
  api.post('/role-applications', {
    reason,
    target_role: targetRole,
  });

export const listMyRoleApplications = () =>
  api.get('/role-applications');


// ── Chat endpoints ───────────────────────────────────────────────────
export const listChatMessages = (streamId, chatToken) =>
  api.get(`/chat/streams/${streamId}/messages`, {
    params: { chat_token: chatToken },
  });

export const getChatTransportUrl = (streamId, chatToken) => {
  if (!chatToken) return null;
  const host = window.location.hostname;
  const quicPort = 4443;
  return `https://${host}:${quicPort}/chat/${streamId}?chat_token=${encodeURIComponent(chatToken)}`;
};

// WebSocket fallback for browsers without WebTransport (Firefox, Safari).
// Same-origin upgrade through the frontend nginx so it inherits the page's
// HTTPS and avoids cross-origin restrictions. Uses the stream-scoped chat
// token (same one used by the WebTransport path).
export const getChatWebSocketUrl = (streamId, chatToken) => {
  if (!chatToken) return null;
  const scheme = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${scheme}//${window.location.host}/api/chat/streams/${streamId}/ws?chat_token=${encodeURIComponent(chatToken)}`;
};


export default api;
