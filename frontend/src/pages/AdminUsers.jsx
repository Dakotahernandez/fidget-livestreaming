import React, { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import useAuth from '../hooks/useAuth';
import {
	listPendingRoleApplications,
	listUsers,
	reviewRoleApplication,
	updateUserRole,
} from '../services/api';

const ROLES = ['viewer', 'streamer'];

function AdminUsers() {
	const { isAuthenticated, user } = useAuth();
	const navigate = useNavigate();

	const [users, setUsers] = useState([]);
	const [loading, setLoading] = useState(true);
	const [savingId, setSavingId] = useState(null);
	const [reviewingId, setReviewingId] = useState(null);
	const [error, setError] = useState('');
	const [message, setMessage] = useState('');
	const [pendingApps, setPendingApps] = useState([]);

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
				const [usersRes, appsRes] = await Promise.all([
					listUsers(),
					listPendingRoleApplications(),
				]);

				if (!cancelled) {
					setUsers(usersRes.data || []);
					setPendingApps(appsRes.data || []);
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

	const handleReviewApplication = async (applicationId, action, userId, targetRole) => {
		setReviewingId(applicationId);
		setError('');
		setMessage('');

		try {
			await reviewRoleApplication(applicationId, action);

			setPendingApps((current) =>
				current.filter((app) => app.id !== applicationId)
			);

			if (action === 'approve') {
				setUsers((current) =>
					current.map((u) =>
						u.id === userId ? { ...u, role: targetRole } : u
					)
				);
				setMessage(`Application approved and user promoted to ${targetRole}.`);
			} else {
				setMessage('Application denied.');
			}
		} catch (err) {
			const msg =
				err.response?.data?.error ||
				err.response?.data?.message ||
				'Could not review role application.';
			setError(msg);
		} finally {
			setReviewingId(null);
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
				Change whether users are viewers or streamers.
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
				<>
					<div className="dashboard-section mt-2">
						<h3>User Roles</h3>
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
													disabled={savingId === u.id || u.role === 'admin'}
													onChange={(e) => handleRoleChange(u.id, e.target.value)}
													style={{
														padding: '0.35rem 0.5rem',
														borderRadius: 'var(--radius)',
														border: '1px solid var(--color-border)',
														background: 'var(--color-bg)',
														color: 'var(--color-text)',
													}}
												>
													{u.role === 'admin' ? (
														<option value="admin">admin</option>
													) : (
														ROLES.map((role) => (
															<option key={role} value={role}>
																{role}
															</option>
														))
													)}
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

					<div className="dashboard-section mt-2">
						<h3>Pending Streamer Applications</h3>

						{pendingApps.length === 0 ? (
							<p className="mt-1" style={{ color: 'var(--color-text-muted)' }}>
								No pending applications.
							</p>
						) : (
							<div style={{ overflowX: 'auto' }}>
								<table style={{ width: '100%', borderCollapse: 'collapse' }}>
									<thead>
										<tr>
											<th style={headerStyle}>User</th>
											<th style={headerStyle}>Requested Role</th>
											<th style={headerStyle}>Reason</th>
											<th style={headerStyle}>Requested</th>
											<th style={headerStyle}>Action</th>
										</tr>
									</thead>
									<tbody>
										{pendingApps.map((app) => (
											<tr key={app.id}>
												<td style={cellStyle}>
													{app.username} ({app.email})
												</td>
												<td style={cellStyle}>{app.target_role}</td>
												<td style={cellStyle}>{app.reason || 'No reason provided.'}</td>
												<td style={cellStyle}>
													{app.created_at ? new Date(app.created_at).toLocaleString() : ''}
												</td>
												<td style={cellStyle}>
													<div style={{ display: 'flex', gap: '0.5rem' }}>
														<button
															className="btn btn--outline"
															type="button"
															disabled={reviewingId === app.id}
															onClick={() => handleReviewApplication(app.id, 'approve', app.user_id, app.target_role)}
														>
															Approve
														</button>
														<button
															className="btn btn--outline"
															type="button"
															disabled={reviewingId === app.id}
															onClick={() => handleReviewApplication(app.id, 'deny', app.user_id, app.target_role)}
														>
															Deny
														</button>
													</div>
												</td>
											</tr>
										))}
									</tbody>
								</table>
							</div>
						)}
					</div>
				</>
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