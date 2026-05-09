import { createContext, useContext, useState, useEffect, useCallback } from 'react';
import { login as apiLogin, register as apiRegister } from '../services/api';
import {
  clearStoredAuth,
  getStoredToken,
  getStoredUser,
  setStoredToken,
  setStoredUser,
} from '../services/authStorage';

const AuthContext = createContext(null);

/**
 * Provides authentication state and helpers to the component tree.
 *
 * Stores auth state in tab-scoped storage and exposes login / register / logout
 * functions so pages don't need to interact with the API layer directly.
 */
export function AuthProvider({ children }) {
  const [token, setToken] = useState(() => getStoredToken());
  const [user, setUser] = useState(() => getStoredUser());

  // Keep tab-scoped storage in sync whenever token or user changes.
  useEffect(() => {
    setStoredToken(token);
  }, [token]);

  useEffect(() => {
    setStoredUser(user);
  }, [user]);

  const loginUser = useCallback(async (username, password) => {
    const res = await apiLogin(username, password);
    setToken(res.data.token);
    setUser({
      id: res.data.user_id,
      username: res.data.username,
      role: res.data.role,
    });
    return res.data;
  }, []);

  const registerUser = useCallback(async (username, password) => {
    const res = await apiRegister(username, password);
    setToken(res.data.token);
    setUser({
      id: res.data.user_id,
      username: res.data.username,
      role: res.data.role,
    });
    return res.data;
  }, []);

  const logout = useCallback(() => {
    setToken(null);
    setUser(null);
    clearStoredAuth();
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
