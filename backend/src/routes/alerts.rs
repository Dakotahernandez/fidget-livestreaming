//! Server-Sent Events (SSE) channel that fans donation alerts out to
//! every active viewer/streamer connection for a given stream id.
//!
//! The webhook handler (see `routes::webhooks`) is the *only* code path
//! allowed to push to subscribers — alerts must never be triggered by a
//! checkout button click or Stripe redirect URL.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::Path,
    response::sse::{Event, KeepAlive, Sse},
    routing::get,
    Extension, Router,
};
use futures_util::stream::Stream;
use sqlx::PgPool;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::config::Config;

/// Per-stream registry of SSE senders. The webhook handler grabs the
/// list for a given `stream_id` and pushes the same JSON payload to every
/// sender. Dead senders are pruned lazily.
pub type AlertSenders = Arc<Mutex<HashMap<Uuid, Vec<mpsc::UnboundedSender<String>>>>>;

/// Build the alerts sub-router.
pub fn router() -> Router<(PgPool, Config)> {
    Router::new().route("/:stream_id", get(subscribe))
}

/// `GET /api/alerts/:stream_id` — open a long-lived SSE connection.
///
/// Each connection gets its own mpsc channel; the sender is parked in
/// `AlertSenders[stream_id]` so the webhook handler can broadcast to it.
/// A 15 second comment ping keeps proxies (nginx, CloudFront) from
/// reaping the idle connection.
async fn subscribe(
    Extension(senders): Extension<AlertSenders>,
    Path(stream_id): Path<Uuid>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    senders.lock().await.entry(stream_id).or_default().push(tx);

    // JSON payloads pushed by the webhook handler.
    let data_stream = UnboundedReceiverStream::new(rx)
        .map(|payload| Ok::<_, Infallible>(Event::default().data(payload)));

    // Periodic comment ping. SSE comments start with `:` and are ignored
    // by the EventSource parser, so they are perfect keepalives.
    let ping_stream = tokio_stream::wrappers::IntervalStream::new(
        tokio::time::interval(Duration::from_secs(15)),
    )
    .map(|_| Ok::<_, Infallible>(Event::default().comment("ping")));

    Sse::new(data_stream.merge(ping_stream)).keep_alive(KeepAlive::default())
}

/// Push a serialized JSON payload to every active subscriber of
/// `stream_id`. Senders whose channels are closed are dropped from the
/// map so the registry doesn't leak.
pub async fn broadcast(senders: &AlertSenders, stream_id: Uuid, payload: &str) {
    let mut guard = senders.lock().await;
    let Some(list) = guard.get_mut(&stream_id) else {
        return;
    };
    // SECURITY: `retain` re-evaluates every element, dropping any sender
    // whose receiver was already torn down. This runs on every broadcast
    // so dead entries cannot accumulate.
    list.retain(|tx| tx.send(payload.to_string()).is_ok());
    if list.is_empty() {
        // SECURITY: free the map entry entirely once all senders are
        // gone — otherwise a long-running server with many one-off
        // streams would leak one empty Vec per stream forever.
        guard.remove(&stream_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_senders() -> AlertSenders {
        Arc::new(Mutex::new(HashMap::new()))
    }

    #[tokio::test]
    async fn broadcast_to_empty_stream_is_noop() {
        let senders = fresh_senders();
        // Should not panic or insert anything.
        broadcast(&senders, Uuid::new_v4(), "{}").await;
        assert!(senders.lock().await.is_empty());
    }

    #[tokio::test]
    async fn broadcast_delivers_to_every_active_subscriber() {
        let senders = fresh_senders();
        let stream_id = Uuid::new_v4();
        let (tx_a, mut rx_a) = mpsc::unbounded_channel::<String>();
        let (tx_b, mut rx_b) = mpsc::unbounded_channel::<String>();
        senders
            .lock()
            .await
            .entry(stream_id)
            .or_default()
            .extend([tx_a, tx_b]);

        broadcast(&senders, stream_id, "hello").await;
        assert_eq!(rx_a.recv().await, Some("hello".to_string()));
        assert_eq!(rx_b.recv().await, Some("hello".to_string()));
    }

    #[tokio::test]
    async fn broadcast_prunes_dead_senders_and_empty_entries() {
        let senders = fresh_senders();
        let stream_id = Uuid::new_v4();
        let (tx_dead, rx_dead) = mpsc::unbounded_channel::<String>();
        drop(rx_dead); // close the receiver immediately
        senders
            .lock()
            .await
            .entry(stream_id)
            .or_default()
            .push(tx_dead);

        broadcast(&senders, stream_id, "x").await;
        // Only sender was dead → entry should be removed entirely.
        assert!(!senders.lock().await.contains_key(&stream_id));
    }

    #[tokio::test]
    async fn broadcast_keeps_live_when_one_subscriber_dies() {
        let senders = fresh_senders();
        let stream_id = Uuid::new_v4();
        let (tx_dead, rx_dead) = mpsc::unbounded_channel::<String>();
        drop(rx_dead);
        let (tx_live, mut rx_live) = mpsc::unbounded_channel::<String>();
        senders
            .lock()
            .await
            .entry(stream_id)
            .or_default()
            .extend([tx_dead, tx_live]);

        broadcast(&senders, stream_id, "alert").await;
        assert_eq!(rx_live.recv().await, Some("alert".to_string()));
        let guard = senders.lock().await;
        assert_eq!(guard.get(&stream_id).map(Vec::len), Some(1));
    }
}
