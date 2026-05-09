import React from 'react';
import { BrowserRouter, Routes, Route, Link } from 'react-router-dom';
import { AuthProvider } from './hooks/useAuth';
import useAuth from './hooks/useAuth';

import Home from './pages/Home';
import Login from './pages/Login';
import Register from './pages/Register';
import Dashboard from './pages/Dashboard';
import Watch from './pages/Watch';
import AdminUsers from './pages/AdminUsers';

/** Top-level nav bar — adapts based on auth state. */
function Navbar() {
  const { isAuthenticated, user, logout } = useAuth();

  return (
    <nav className="navbar">
      <Link to="/" className="navbar__brand">
        <img
          src="/fidget-icon.svg"
          alt="Fidget"
          className="navbar__brand-icon"
        />
        <span className="navbar__brand-text">FIDGET</span>
      </Link>
      <div className="navbar__links">
        <Link to="/">Home</Link>
        {isAuthenticated ? (
          <>
            {user?.role === 'admin' && (
              <Link to="/admin/users">Manage Users</Link>
            )}
            <Link to="/dashboard">Dashboard</Link>
            <span style={{ color: 'var(--color-text-secondary)', fontSize: '0.85rem' }}>
              {user?.username}
            </span>
            <button type="button" onClick={logout}>Log Out</button>
          </>
        ) : (
          <>
            <Link to="/login">Log In</Link>
            <Link to="/register">Register</Link>
          </>
        )}
      </div>
    </nav>
  );
}

function App() {
  return (
    <BrowserRouter>
      <AuthProvider>
        <Navbar />
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/login" element={<Login />} />
          <Route path="/register" element={<Register />} />
          <Route path="/dashboard" element={<Dashboard />} />
          <Route path="/watch/:id" element={<Watch />} />
          <Route path="/admin/users" element={<AdminUsers />} />
        </Routes>
      </AuthProvider>
    </BrowserRouter>
  );
}

export default App;
