//! Stripe webhook receiver. The webhook is the *only* trigger for
//! donation alerts and the only place the `donations.status` flips
//! from `pending` to `completed`.
//!
//! Idempotency is enforced with a single-statement
//! `UPDATE … WHERE status='pending' RETURNING *`: a redelivered event
//! returns 0 rows and never broadcasts twice.

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Extension, Router,
};
use serde_json::json;
use sqlx::PgPool;
use stripe::{EventObject, EventType, Webhook};
use uuid::Uuid;

use crate::config::Config;
use crate::routes::alerts::{self, AlertSenders};
use crate::routes::sounds::file_path_to_url;

/// Build the Stripe webhook sub-router.
pub fn router() -> Router<(PgPool, Config)> {
    Router::new().route("/webhook", post(stripe_webhook))
}

/// `POST /api/stripe/webhook` — verify the Stripe-Signature header and
/// process `checkout.session.completed` events. We always return 200 so
/// Stripe doesn't pile up retries for events we've intentionally
/// ignored (unknown types, replays, missing rows).
async fn stripe_webhook(
    State((pool, cfg)): State<(PgPool, Config)>,
    Extension(senders): Extension<AlertSenders>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let signature = headers
        .get("Stripe-Signature")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();

    let payload = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!("stripe webhook: non-utf8 body: {err}");
            return (StatusCode::OK, "ok");
        }
    };

    let event = match Webhook::construct_event(payload, signature, &cfg.stripe_webhook_secret) {
        Ok(ev) => ev,
        Err(err) => {
            tracing::warn!("stripe webhook signature verification failed: {err:?}");
            return (StatusCode::OK, "ok");
        }
    };

    if event.type_ != EventType::CheckoutSessionCompleted {
        return (StatusCode::OK, "ok");
    }

    let EventObject::CheckoutSession(session) = event.data.object else {
        return (StatusCode::OK, "ok");
    };

    let session_id = session.id.to_string();
    let payment_intent = session
        .payment_intent
        .as_ref()
        .map(|pi| pi.id().to_string());

    // Idempotent flip: only the first delivery for a given session id
    // updates a row, so replays return 0 rows and we skip broadcasting.
    let updated: Option<(Uuid, String, i32, Option<Uuid>)> = match sqlx::query_as(
        "UPDATE donations \
         SET status = 'completed', stripe_payment_intent = $2 \
         WHERE stripe_session_id = $1 AND status = 'pending' \
         RETURNING stream_id, donor_name, amount_cents, sound_id",
    )
    .bind(&session_id)
    .bind(payment_intent.as_deref())
    .fetch_optional(&pool)
    .await
    {
        Ok(row) => row,
        Err(err) => {
            tracing::error!("stripe webhook db update failed: {err:?}");
            return (StatusCode::OK, "ok");
        }
    };

    let Some((stream_id, donor_name, amount_cents, sound_id)) = updated else {
        // Either we never saw this session (donations row missing) or
        // it was already marked completed. Either way, do nothing.
        return (StatusCode::OK, "ok");
    };

    // Resolve the sound's public URL if one was selected.
    let sound_url = match sound_id {
        Some(sid) => sqlx::query_scalar::<_, String>(
            "SELECT file_path FROM donation_sounds WHERE id = $1",
        )
        .bind(sid)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten()
        .map(|p| file_path_to_url(&p)),
        None => None,
    };

    let payload = json!({
        "type": "donation_alert",
        "donor_name": donor_name,
        "amount_cents": amount_cents,
        "sound_url": sound_url,
    })
    .to_string();

    alerts::broadcast(&senders, stream_id, &payload).await;

    (StatusCode::OK, "ok")
}
