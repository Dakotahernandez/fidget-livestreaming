use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use crate::models::chat;

//Max number of buffered messages per stream channel.
const CHAT_CHANNEL_CAPACITY: usize = 100;

//Max bytes read from the WebTransport stream in one poll.
const CHAT_READ_BUF: usize = 4096;

/// Per-stream broadcast channels keyed by stream UUID.
#[derive(Clone)]
pub struct ChatState {
    channels: Arc<Mutex<HashMap<Uuid, broadcast::Sender<chat::ChatMessage>>>>,  //stream_id → fan-out channel
}

impl ChatState {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn sender_for_stream(&self, stream_id: Uuid) -> broadcast::Sender<chat::ChatMessage> {
        let mut channels = self.channels.lock().await;

        channels
            .entry(stream_id)
            .or_insert_with(|| {
                let (sender, _receiver) = broadcast::channel(CHAT_CHANNEL_CAPACITY);
                sender
            })
            .clone()
    }
}

//Hard cap on chat message length. DB enforces 1...500; we mirror it here.
pub const MAX_CHAT_MESSAGE_CHARS: usize = 500;

//Wire format from the client: a single JSON object per line.
#[derive(Debug, Deserialize)]
pub struct IncomingChatMessage {
    pub message: String,  //the chat text the user typed
}

/// Validate, persist, and fan-out one incoming chat line.
pub async fn process_incoming_line(
    pool: &sqlx::PgPool,
    channel: &broadcast::Sender<chat::ChatMessage>,
    stream_id: Uuid,
    user_id: Uuid,
    line: &str,
) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }

    let incoming: IncomingChatMessage = match serde_json::from_str(line) {
        Ok(parsed) => parsed,
        Err(e) => {
            tracing::warn!(
                stream_id = %stream_id,
                user_id = %user_id,
                "Invalid chat JSON: {e}"
            );
            return;
        }
    };

    let message = incoming.message.trim();
    if message.is_empty() {
        return;
    }

    if message.chars().count() > MAX_CHAT_MESSAGE_CHARS {
        tracing::warn!(
            stream_id = %stream_id,
            user_id = %user_id,
            "Rejected chat message over {MAX_CHAT_MESSAGE_CHARS} characters"
        );
        return;
    }

    match chat::insert_message(pool, stream_id, user_id, message).await {
        Ok(saved) => {
            let _ = channel.send(saved);
        }
        Err(e) => {
            tracing::error!(
                stream_id = %stream_id,
                user_id = %user_id,
                "Failed to save chat message: {e}"
            );
        }
    }
}

/// Parse the WebTransport chat path into its parts.
pub fn parse_chat_path(path: &str) -> Option<(Uuid, String)> {
    let (path_part, query) = path.split_once('?').unwrap_or((path, ""));

    let stream_id_str = path_part.strip_prefix("/chat/")?;
    let stream_id = Uuid::parse_str(stream_id_str).ok()?;

    let token = query
        .split('&')
        .find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
			if key == "chat_token" || key == "token" {
                Some(value.to_string())
            } else {
                None
            }
        })?;

    Some((stream_id, token))
}

/// Drive one WebTransport chat session end-to-end.
pub async fn handle_chat_session(
	connection: wtransport::Connection,
	pool: sqlx::PgPool,
	stream_id: Uuid,
	user_id: Uuid,
	chat_state: ChatState,
) {
	tracing::info!(
		stream_id = %stream_id,
		user_id = %user_id,
		"WebTransport chat session accepted"
	);

	let channel = chat_state.sender_for_stream(stream_id).await;
	let mut receiver = channel.subscribe();

	let (mut send_stream, mut recv_stream) = match connection.accept_bi().await {
		Ok(streams) => streams,
		Err(e) => {
			tracing::warn!(
				stream_id = %stream_id,
				user_id = %user_id,
				"Failed to accept chat bidirectional stream: {e}"
			);
			return;
		}
	};

	//Server -> client: every saved message goes out as a newline-terminated JSON line.
	let writer = tokio::spawn(async move {
		let mut keep_writing = true;

		while keep_writing {
			match receiver.recv().await {
				Ok(msg) => {
					match serde_json::to_string(&msg) {
						Ok(json) => {
							let payload = format!("{json}\n");

							if let Err(e) = send_stream.write_all(payload.as_bytes()).await {
								tracing::warn!("Failed to write chat message to client: {e}");
								keep_writing = false;
							}
						}
						Err(e) => {
							tracing::warn!("Failed to serialize chat message: {e}");
						}
					}
				}
				Err(broadcast::error::RecvError::Lagged(_)) => {
					//Lagged subscriber. Skip - viewer will refetch via REST.
				}
				Err(broadcast::error::RecvError::Closed) => {
					keep_writing = false;
				}
			}
		}
	});

	//Client -> server: read raw bytes and split on '\n'. Each line is one JSON message.
	let mut buf = vec![0u8; CHAT_READ_BUF];
	let mut pending = String::new();
	let mut keep_reading = true;

	while keep_reading {
		match recv_stream.read(&mut buf).await {
			Ok(Some(0)) | Ok(None) => {
				tracing::info!(
					stream_id = %stream_id,
					user_id = %user_id,
					"Chat stream ended"
				);

				keep_reading = false;
			}
			Ok(Some(n)) => {
				let chunk = String::from_utf8_lossy(&buf[..n]);
				pending.push_str(&chunk);

				while let Some(newline_index) = pending.find('\n') {
					let line: String = pending.drain(..=newline_index).collect();
					process_incoming_line(&pool, &channel, stream_id, user_id, &line).await;
				}
			}
			Err(e) => {
				tracing::warn!(
					stream_id = %stream_id,
					user_id = %user_id,
					"Chat stream read error: {e}"
				);

				keep_reading = false;
			}
		}
	}

	writer.abort();

	tracing::info!(
		stream_id = %stream_id,
		user_id = %user_id,
		"WebTransport chat session closed"
	);
}