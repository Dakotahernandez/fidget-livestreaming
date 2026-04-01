import { createContext, useContext, useState, useEffect, useCallback } from 'react';
import { login as apiLogin, register as apiRegister } from '../services/api';

const AuthContext = createContext(null);

/**
 * Provides authentication state and helpers to the component tree.
 *
 * Stores the JWT in localStorage and exposes login / register / logout
 * functions so pages don't need to interact with the API layer directly.
 */
export function AuthProvider({ children }) {
  const [token, setToken] = useState(() => localStorage.getItem('token'));
  const [user, setUser] = useState(() => {
    try {
      const stored = localStorage.getItem('user');
      return stored ? JSON.parse(stored) : null;
    } catch {
      return null;
    }
  });

  // Keep localStorage in sync whenever token or user changes.
  useEffect(() => {
    if (token) {
      localStorage.setItem('token', token);
    } else {
      localStorage.removeItem('token');
    }
  }, [token]);

  useEffect(() => {
    if (user) {
      localStorage.setItem('user', JSON.stringify(user));
    } else {
      localStorage.removeItem('user');
    }
  }, [user]);

  const loginUser = useCallback(async (username, password) => {
    const res = await apiLogin(username, password);
    setToken(res.data.token);
    setUser({ username, id: res.data.user_id });
    return res.data;
  }, []);

  const registerUser = useCallback(async (username, password) => {
    const res = await apiRegister(username, password);
    setToken(res.data.token);
    setUser({ username, id: res.data.user_id });
    return res.data;
  }, []);

  const logout = useCallback(() => {
    setToken(null);
    setUser(null);
  }, []);

  const value = {
    token,
    user,
    isAuthenticated: !!token,
    login: loginUser,
    register: registerUser,
    logout,
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

/**
 * Convenience hook — call from any component inside <AuthProvider>.
 */
export default function useAuth() {
  const ctx = useContext(AuthContext);
  if (!ctx) {
    throw new Error('useAuth must be used within an <AuthProvider>');
  }
  return ctx;
}
