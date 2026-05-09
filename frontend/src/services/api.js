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

// ── Admin endpoints ─────────────────────────────────────────────────
export const listUsers = () =>
  api.get('/admin/users');

export const updateUserRole = (id, role) =>
  api.patch(`/admin/users/${id}/role`, { role });


// ── Chat endpoints ───────────────────────────────────────────────────
export const listChatMessages = (streamId) =>
  api.get(`/chat/streams/${streamId}/messages`);

export const getChatTransportUrl = (streamId) => {
  const token = getStoredToken();
  const host = window.location.hostname;
  const quicPort = 4443;
  return `https://${host}:${quicPort}/chat/${streamId}?token=${token}`;
};


export default api;
