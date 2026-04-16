import axios from 'axios';

const api = axios.create({
  baseURL: '/api',
});

// ── JWT interceptor ──────────────────────────────────────────────────
// Attach the stored token to every outgoing request automatically.
api.interceptors.request.use((config) => {
  const token = localStorage.getItem('token');
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
      localStorage.removeItem('token');
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
export const createStream = (title) =>
  api.post('/streams', { title });

export const listStreams = () =>
  api.get('/streams');

export const getStream = (id) =>
  api.get(`/streams/${id}`);

export const startStream = (id) =>
  api.put(`/streams/${id}/start`);

export const stopStream = (id) =>
  api.put(`/streams/${id}/stop`);

export const getCertHash = () =>
  api.get('/transport/cert-hash');

export const getTransportUrl = (streamId) => {
  const token = localStorage.getItem('token');
  const host = window.location.hostname;
  const quicPort = 4443;
  return `https://${host}:${quicPort}/ingest/${streamId}?token=${token}`;
};

export default api;
