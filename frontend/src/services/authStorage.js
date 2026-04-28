const TOKEN_KEY = 'token';
const USER_KEY = 'user';

function getStorage() {
  if (typeof window === 'undefined') {
    return null;
  }

  try {
    return window.sessionStorage;
  } catch {
    return window.localStorage;
  }
}

export function getStoredToken() {
  const storage = getStorage();
  if (!storage) return null;
  return storage.getItem(TOKEN_KEY);
}

export function setStoredToken(token) {
  const storage = getStorage();
  if (!storage) return;

  if (token) {
    storage.setItem(TOKEN_KEY, token);
  } else {
    storage.removeItem(TOKEN_KEY);
  }
}

export function getStoredUser() {
  const storage = getStorage();
  if (!storage) return null;

  const raw = storage.getItem(USER_KEY);
  if (!raw) return null;

  try {
    return JSON.parse(raw);
  } catch {
    storage.removeItem(USER_KEY);
    return null;
  }
}

export function setStoredUser(user) {
  const storage = getStorage();
  if (!storage) return;

  if (user) {
    storage.setItem(USER_KEY, JSON.stringify(user));
  } else {
    storage.removeItem(USER_KEY);
  }
}

export function clearStoredAuth() {
  const storage = getStorage();
  if (!storage) return;

  storage.removeItem(TOKEN_KEY);
  storage.removeItem(USER_KEY);
}
