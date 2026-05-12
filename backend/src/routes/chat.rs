use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    middleware as axum_mw,
    response::Response,
    routing::get,
    Extension, Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::errors::AppError;
use crate::middleware::auth::{decode_chat_access_token, require_auth, AuthUser};
use crate::models::{chat, stream};
use crate::quic_chat::{self, ChatState};

//Builds the /api/chat/* sub-router.
pub fn router() -> Router<(PgPool, Config)> {
    // `list_messages` is wrapped in the standard session-auth middleware
    // (and additionally re-checks a chat-scoped token from the query
    // string). The WebSocket route can NOT live behind that middleware
    // because browsers don't allow custom headers on `new WebSocket(...)`
    // — so it validates its own `?token=` chat token inside the handler
    // before completing the upgrade.
    let authed = Router::new()
        .route("/streams/:id/messages", get(list_messages))
        .layer(axum_mw::from_fn(require_auth));

    let ws = Router::new().route("/streams/:id/ws", get(chat_ws));

    authed.merge(ws)
}

#[derive(Debug, Deserialize)]
struct ChatTokenQuery {
    #[serde(alias = "token")]
    chat_token: String,
}

//Query string for the WS endpoint: just the JWT.
#[derive(Debug, Deserialize)]
struct WsTokenQuery {
    #[serde(alias = "chat_token")]
    token: String,
}

/// `GET /api/chat/streams/:id/messages` - last 50 messages for a stream, oldest first.
async fn list_messages(
    State((pool, config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(params): Query<ChatTokenQuery>,
) -> Result<Json<Vec<Value>>, AppError> {
    let chat_claims = decode_chat_access_token(&params.chat_token, &config.jwt_secret)?;

    if chat_claims.sub != auth.user_id {
        return Err(AppError::Unauthorized(
            "chat token does not match authenticated user".to_string(),
        ));
    }

    if chat_claims.stream_id != id {
        return Err(AppError::Unauthorized(
            "chat token does not grant access to this stream".to_string(),
        ));
    }

    let _stream = stream::find_by_id(&pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    let messages = chat::list_recent_messages(&pool, id, 50).await?;

    let payload: Vec<Value> = messages
        .into_iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "stream_id": m.stream_id,
                "user_id": m.user_id,
                "username": m.username,
                "message": m.message,
                "created_at": m.created_at,
            })
        })
        .collect();

    Ok(Json(payload))
}

/// GET /api/chat/streams/:id/ws?token=<jwt> - WebSocket chat fallback.
/// For browsers without WebTransport (Firefox, Safari). Same line-delimited
/// JSON protocol as the QUIC chat handler: client sends { "message": "..." },
/// server broadcasts full `ChatMessage` JSON to every subscriber.
async fn chat_ws(
    ws: WebSocketUpgrade,
    Path(stream_id): Path<Uuid>,
    Query(WsTokenQuery { token }): Query<WsTokenQuery>,
    Extension(chat_state): Extension<ChatState>,
    State((pool, config)): State<(PgPool, Config)>,
) -> Result<Response, AppError> {
    let chat_claims = decode_chat_access_token(&token, &config.jwt_secret)?;

    if chat_claims.stream_id != stream_id {
        return Err(AppError::Unauthorized(
            "chat token does not grant access to this stream".to_string(),
        ));
    }

    let user_id = chat_claims.sub;

    //Confirm the stream exists before upgrading. Cheaper than failing inside the WS task.
    let _stream = stream::find_by_id(&pool, stream_id)
        .await?
        .ok_or_else(|| AppError::NotFound("stream not found".to_string()))?;

    Ok(ws.on_upgrade(move |socket| {
        handle_ws_session(socket, pool, chat_state, stream_id, user_id)
    }))
}

/// Pumps the per-stream broadcast channel in both directions for one WS peer.
async fn handle_ws_session(
    socket: WebSocket,
    pool: PgPool,
    chat_state: ChatState,
    stream_id: Uuid,
    user_id: Uuid,
) {
    tracing::info!(
        stream_id = %stream_id,
        user_id = %user_id,
        "WebSocket chat session accepted"
    );

    let channel = chat_state.sender_for_stream(stream_id).await;
    let mut receiver = channel.subscribe();

    let (mut ws_sink, mut ws_stream) = socket.split();

    //Server -> client: every saved message goes out as a JSON text frame.
    let writer = tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(msg) => match serde_json::to_string(&msg) {
                    Ok(json) => {
                        if ws_sink.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to serialize chat message: {e}");
                    }
                },
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    //Client -> server: each text/binary frame is one JSON message.
    while let Some(frame) = ws_stream.next().await {
        let line = match frame {
            Ok(Message::Text(text)) => text,
            Ok(Message::Binary(bytes)) => match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => continue,
            },
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            Ok(Message::Close(_)) => break,
            Err(e) => {
                tracing::warn!(
                    stream_id = %stream_id,
                    user_id = %user_id,
                    "WebSocket chat read error: {e}"
                );
                break;
            }
        };

        quic_chat::process_incoming_line(&pool, &channel, stream_id, user_id, &line).await;
    }

    writer.abort();

    tracing::info!(
        stream_id = %stream_id,
        user_id = %user_id,
        "WebSocket chat session closed"
    );
}
