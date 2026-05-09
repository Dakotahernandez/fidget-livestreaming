import React, { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import useAuth from '../hooks/useAuth';
import { listUsers, updateUserRole } from '../services/api';

const ROLES = ['viewer', 'streamer', 'admin'];

function AdminUsers() {
	const { isAuthenticated, user } = useAuth();
	const navigate = useNavigate();

	const [users, setUsers] = useState([]);
	const [loading, setLoading] = useState(true);
	const [savingId, setSavingId] = useState(null);
	const [error, setError] = useState('');
	const [message, setMessage] = useState('');

	const isAdmin = user?.role === 'admin';

	useEffect(() => {
		if (!isAuthenticated) {
			navigate('/login');
		}
	}, [isAuthenticated, navigate]);

	useEffect(() => {
		if (!isAuthenticated || !isAdmin) {
			setLoading(false);
			return;
		}

		let cancelled = false;

		const fetchUsers = async () => {
			try {
				setError('');
				const res = await listUsers();

				if (!cancelled) {
					setUsers(res.data || []);
				}
			} catch (err) {
				if (!cancelled) {
					const msg =
						err.response?.data?.error ||
						err.response?.data?.message ||
						'Could not load users.';
					setError(msg);
				}
			} finally {
				if (!cancelled) {
					setLoading(false);
				}
			}
		};

		fetchUsers();

		return () => {
			cancelled = true;
		};
	}, [isAuthenticated, isAdmin]);

	const handleRoleChange = async (id, nextRole) => {
		setSavingId(id);
		setError('');
		setMessage('');

		try {
			const res = await updateUserRole(id, nextRole);
			const updated = res.data;

			setUsers((current) =>
				current.map((u) =>
					u.id === id ? { ...u, role: updated.role } : u
				)
			);

			setMessage('User role updated.');
		} catch (err) {
			const msg =
				err.response?.data?.error ||
				err.response?.data?.message ||
				'Could not update user role.';
			setError(msg);
		} finally {
			setSavingId(null);
		}
	};

	if (!isAuthenticated) return null;

	if (!isAdmin) {
		return (
			<div className="page">
				<h2>Admin</h2>
				<p className="error-msg mt-1">
					You do not have permission to view this page.
				</p>
			</div>
		);
	}

	return (
		<div className="page">
			<h2>Manage Users</h2>

			<p className="mt-1" style={{ color: 'var(--color-text-muted)' }}>
				Change whether users are viewers, streamers, or admins.
			</p>

			{error && <p className="error-msg mt-1">{error}</p>}

			{message && (
				<p className="mt-1" style={{ color: 'var(--color-text-muted)' }}>
					{message}
				</p>
			)}

			{loading ? (
				<p className="text-center mt-2">Loading users…</p>
			) : (
				<div className="dashboard-section mt-2">
					<div style={{ overflowX: 'auto' }}>
						<table style={{ width: '100%', borderCollapse: 'collapse' }}>
							<thead>
								<tr>
									<th style={headerStyle}>Username</th>
									<th style={headerStyle}>Email</th>
									<th style={headerStyle}>Role</th>
									<th style={headerStyle}>Created</th>
								</tr>
							</thead>

							<tbody>
								{users.map((u) => (
									<tr key={u.id}>
										<td style={cellStyle}>{u.username}</td>
										<td style={cellStyle}>{u.email}</td>
										<td style={cellStyle}>
											<select
												value={u.role}
												disabled={savingId === u.id}
												onChange={(e) => handleRoleChange(u.id, e.target.value)}
												style={{
													padding: '0.35rem 0.5rem',
													borderRadius: 'var(--radius)',
													border: '1px solid var(--color-border)',
													background: 'var(--color-bg)',
													color: 'var(--color-text)',
												}}
											>
												{ROLES.map((role) => (
													<option key={role} value={role}>
														{role}
													</option>
												))}
											</select>
										</td>
										<td style={cellStyle}>
											{u.created_at
												? new Date(u.created_at).toLocaleString()
												: ''}
										</td>
									</tr>
								))}
							</tbody>
						</table>
					</div>

					{users.length === 0 && (
						<p className="text-center mt-2" style={{ color: 'var(--color-text-muted)' }}>
							No users found.
						</p>
					)}
				</div>
			)}
		</div>
	);
}

const headerStyle = {
	textAlign: 'left',
	padding: '0.75rem',
	borderBottom: '1px solid var(--color-border)',
	color: 'var(--color-text-muted)',
	fontSize: '0.85rem',
};

const cellStyle = {
	padding: '0.75rem',
	borderBottom: '1px solid var(--color-border)',
};

export default AdminUsers;