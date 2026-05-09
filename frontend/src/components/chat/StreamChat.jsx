import React, { useEffect, useRef, useState } from 'react';
import {
	getCertHash,
	getChatTransportUrl,
	listChatMessages,
} from '../../services/api';

function hexToBytes(hex) {
	const bytes = new Uint8Array(hex.length / 2);

	for (let i = 0; i < bytes.length; i += 1) {
		bytes[i] = parseInt(hex.substring(i * 2, i * 2 + 2), 16);
	}

	return bytes;
}

function StreamChat({ streamId }) {
	const [messages, setMessages] = useState([]);
	const [input, setInput] = useState('');
	const [connected, setConnected] = useState(false);
	const [error, setError] = useState('');

	const writerRef = useRef(null);
	const messagesEndRef = useRef(null);
	const chatBoxRef = useRef(null);
	const shouldAutoScrollRef = useRef(true);

	useEffect(() => {
	    let cancelled = false;

		const loadMessages = async () => {
		    try {
		    	const res = await listChatMessages(streamId);

				if (!cancelled) {
					setMessages(res.data || []);
				}
			} catch {
				if (!cancelled) {
					setError('Could not load chat messages.');
				}
			}
		};

		loadMessages();

		return () => {
			cancelled = true;
		};
	}, [streamId]);

	useEffect(() => {
		let cancelled = false;
		let reader = null;
		let writer = null;
		let transport = null;

		const connectChat = async () => {
			try {
				setError('');
				setConnected(false);

				if (!window.WebTransport) {
					setError('WebTransport is not supported in this browser.');
					return;
				}

				const url = getChatTransportUrl(streamId);

				if (!url) {
					setError('Log in to use live chat.');
					return;
				}

				const certRes = await getCertHash();
				const certHash = certRes.data.hash;

				transport = new WebTransport(url, {
					serverCertificateHashes: [
						{
							algorithm: 'sha-256',
							value: hexToBytes(certHash),
						},
					],
				});

				await transport.ready;

				if (cancelled) return;

				const stream = await transport.createBidirectionalStream();

				writer = stream.writable.getWriter();
				reader = stream.readable.getReader();

				writerRef.current = writer;
				setConnected(true);

				const decoder = new TextDecoder();
				let pending = '';

				while (!cancelled) {
					const result = await reader.read();

					if (result.done) {
						break;
					}

					pending += decoder.decode(result.value, { stream: true });

					let newlineIndex = pending.indexOf('\n');

					while (newlineIndex !== -1) {
						const line = pending.slice(0, newlineIndex).trim();
						pending = pending.slice(newlineIndex + 1);

						if (line.length > 0) {
							try {
								const message = JSON.parse(line);

								setMessages((current) => {
									const alreadyExists = current.some((m) => m.id === message.id);

									if (alreadyExists) {
										return current;
									}

									return [...current, message];
								});
							} catch {
								// Ignore invalid messages from the server.
							}
						}

						newlineIndex = pending.indexOf('\n');
					}
				}
			} catch (err) {
				console.error('WebTransport chat failed:', err);

				if (!cancelled) {
					setError('Could not connect to live chat.');
					setConnected(false);
				}
			}
		};

		connectChat();

		return () => {
			cancelled = true;
			setConnected(false);

			try {
				reader?.releaseLock();
			} catch {
				// Ignore cleanup errors.
			}

			try {
				writer?.releaseLock();
			} catch {
				// Ignore cleanup errors.
			}

			try {
				transport?.close();
			} catch {
				// Ignore cleanup errors.
			}

			writerRef.current = null;
		};
	}, [streamId]);

	useEffect(() => {
		if (shouldAutoScrollRef.current) {
			messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
		}
	}, [messages]);

	const handleChatScroll = () => {
		const chatBox = chatBoxRef.current;

		if (!chatBox) {
			return;
		}

		const distanceFromBottom =
			chatBox.scrollHeight - chatBox.scrollTop - chatBox.clientHeight;

		shouldAutoScrollRef.current = distanceFromBottom < 80;
	};

	const sendMessage = async (e) => {
		e.preventDefault();

		const trimmed = input.trim();

		if (!trimmed || !writerRef.current) {
			return;
		}

		if (trimmed.length > 500) {
			setError('Messages must be 500 characters or less.');
			return;
		}

		try {
			const encoder = new TextEncoder();
			const payload = JSON.stringify({ message: trimmed }) + '\n';

			await writerRef.current.write(encoder.encode(payload));

			setInput('');
			setError('');
		} catch {
			setError('Could not send message.');
		}
	};

	const statusText = connected ? 'Connected' : error ? 'Disconnected' : 'Connecting...';

	return (
		<div className="dashboard-section mt-2">
			<h3>Chat</h3>

			<p style={{ color: 'var(--color-text-muted)', fontSize: '0.85rem' }}>
				Status: {statusText}
			</p>

			{error && <p className="error-msg mt-1">{error}</p>}

			<div
				ref={chatBoxRef}
				onScroll={handleChatScroll}
				style={{
					border: '1px solid var(--color-border)',
					borderRadius: 'var(--radius)',
					padding: '0.75rem',
					marginTop: '0.75rem',
					minHeight: '180px',
					maxHeight: '280px',
					overflowY: 'auto',
					overflowX: 'hidden',
					background: 'var(--color-bg)',
					minWidth: 0,
					maxWidth: '100%',
				}}
			>
				{messages.length === 0 ? (
					<p style={{ color: 'var(--color-text-muted)' }}>
						No messages yet.
					</p>
				) : (
					messages.map((msg) => (
						<div
							key={msg.id}
							style={{
								marginBottom: '0.5rem',
								maxWidth: '100%',
								minWidth: 0,
								overflowWrap: 'anywhere',
								wordBreak: 'break-word',
								whiteSpace: 'normal',
							}}
						>
							<strong>{msg.username || 'User'}:</strong>{' '}
							<span
								style={{
									overflowWrap: 'anywhere',
									wordBreak: 'break-word',
									whiteSpace: 'normal',
								}}
							>
								{msg.message}
							</span>
						</div>
					))
				)}

				<div ref={messagesEndRef} />
			</div>

			<form
				onSubmit={sendMessage}
				style={{
					display: 'flex',
					gap: '0.5rem',
					marginTop: '0.75rem',
					minWidth: 0,
					maxWidth: '100%',
				}}
			>
				<input
					type="text"
					placeholder="Send a message..."
					value={input}
					onChange={(e) => setInput(e.target.value)}
					maxLength={500}
					disabled={!connected}
					style={{
						flex: 1,
						minWidth: 0,
						padding: '0.5rem 0.75rem',
						background: 'var(--color-bg)',
						border: '1px solid var(--color-border)',
						borderRadius: 'var(--radius)',
						color: 'var(--color-text)',
					}}
				/>

				<button className="btn" type="submit" disabled={!connected}>
					Send
				</button>
			</form>
		</div>
	);
}

export default StreamChat;