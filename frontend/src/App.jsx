import React from 'react';
import { BrowserRouter, Routes, Route, Link } from 'react-router-dom';
import Home from './pages/Home';
import Dashboard from './pages/Dashboard';
import Login from './pages/Login';
import Register from './pages/Register';
import Watch from './pages/Watch';

function App() {
  return (
    <BrowserRouter>
      <nav style={{ padding: '12px 20px', background: '#1a1a2e', display: 'flex', gap: 16, alignItems: 'center' }}>
        <Link to="/" style={{ color: '#fff', textDecoration: 'none', fontWeight: 700, fontSize: 20 }}>Fidget</Link>
        <Link to="/dashboard" style={{ color: '#ccc', textDecoration: 'none' }}>Dashboard</Link>
        <Link to="/login" style={{ color: '#ccc', textDecoration: 'none', marginLeft: 'auto' }}>Login</Link>
        <Link to="/register" style={{ color: '#ccc', textDecoration: 'none' }}>Register</Link>
      </nav>
      <Routes>
        <Route path="/" element={<Home />} />
        <Route path="/dashboard" element={<Dashboard />} />
        <Route path="/login" element={<Login />} />
        <Route path="/register" element={<Register />} />
        <Route path="/watch/:streamKey" element={<Watch />} />
      </Routes>
    </BrowserRouter>
  );
}

export default App;
