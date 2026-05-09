//! Stripe Checkout session creation for donations + a streamer-facing
//! donation history endpoint.
//!
//! No alerts are fired here — a `donations` row is inserted with
//! `status = 'pending'` and the row only flips to `'completed'` when
//! the Stripe webhook (see `routes::webhooks`) confirms the payment.

use axum::{
    extract::{Query, State},
    middleware as axum_mw,
    routing::{get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;
use std::str::FromStr;
use stripe::{
    CheckoutSession, CheckoutSessionMode, Client, CreateCheckoutSession,
    CreateCheckoutSessionLineItems, CreateCheckoutSessionLineItemsPriceData,
    CreateCheckoutSessionLineItemsPriceDataProductData, CreateCheckoutSessionPaymentIntentData,
    CreateCheckoutSessionPaymentIntentDataTransferData, Currency,
};
use uuid::Uuid;

use crate::config::Config;
use crate::errors::AppError;
use crate::middleware::auth::{require_auth, AuthUser};

/// Build the donations sub-router. Checkout creation is public (donors
/// don't need accounts); history requires the streamer's JWT.
pub fn router() -> Router<(PgPool, Config)> {
    let public = Router::new().route("/checkout", post(create_checkout));
    let protected = Router::new()
        .route("/", get(history))
        .layer(axum_mw::from_fn(require_auth));
    public.merge(protected)
}

#[derive(Debug, Deserialize)]
struct CheckoutRequest {
    stream_id: Uuid,
    donor_name: String,
    amount_cents: i64,
    sound_id: Option<Uuid>,
}

/// Minimum donation enforced both client- and server-side (Stripe's own
/// minimum is $0.50; we round up to a friendlier $1.00).
pub const MIN_DONATION_CENTS: i64 = 100;

/// SECURITY: Hard server-side ceiling on a single donation. Without
/// this an attacker (or a misbehaving client) can submit absurd
/// amounts that pass through to Stripe and inflate metrics or trigger
/// fraud-review noise. $10,000 is well above any legitimate single tip.
pub const MAX_DONATION_CENTS: i64 = 1_000_000;

/// SECURITY: Server-side cap on `donor_name` so a hostile donor can't
/// blow out the SSE payload, the on-screen overlay layout, or the
/// `donations.donor_name` column with a megabyte string.
pub const MAX_DONOR_NAME_LEN: usize = 100;

/// Platform cut of the donation, in basis points of 1%. The streamer
/// gets the remaining 85%; Stripe processing fees come out of the
/// platform's 15%.
pub const PLATFORM_FEE_PERCENT: i64 = 15;

/// Compute the application fee Stripe should pull on our behalf.
/// Extracted so unit tests can pin the 85/15 split without spinning
/// up Stripe.
pub fn application_fee_amount(amount_cents: i64) -> i64 {
    amount_cents * PLATFORM_FEE_PERCENT / 100
}

/// Validate donor-supplied checkout fields. Returns the trimmed,
/// length-capped donor name on success or a `BadRequest` describing the
/// first failure. Pure function so the test suite can exercise every
/// edge of the amount and name validation without a database.
pub fn validate_checkout_input(
    donor_name_raw: &str,
    amount_cents: i64,
) -> Result<String, AppError> {
    let donor_name = donor_name_raw.trim().to_string();
    if donor_name.is_empty() {
        return Err(AppError::BadRequest("donor_name is required".to_string()));
    }
    if donor_name.chars().count() > MAX_DONOR_NAME_LEN {
        return Err(AppError::BadRequest(format!(
            "donor_name must be {MAX_DONOR_NAME_LEN} characters or fewer"
        )));
    }
    if amount_cents < MIN_DONATION_CENTS {
        return Err(AppError::BadRequest(format!(
            "minimum donation is {MIN_DONATION_CENTS} cents"
        )));
    }
    if amount_cents > MAX_DONATION_CENTS {
        return Err(AppError::BadRequest(format!(
            "maximum donation is {MAX_DONATION_CENTS} cents"
        )));
    }
    Ok(donor_name)
}

/// `POST /api/donations/checkout` — create a Stripe Checkout Session
/// and return the hosted-checkout URL the donor should be redirected to.
async fn create_checkout(
    State((pool, cfg)): State<(PgPool, Config)>,
    Json(body): Json<CheckoutRequest>,
) -> Result<Json<Value>, AppError> {
    // SECURITY: All bounds checks live in `validate_checkout_input` so
    // no checkout request reaches Stripe (or our donations table) with
    // an out-of-range amount or untrimmed/oversized donor name.
    let donor_name = validate_checkout_input(&body.donor_name, body.amount_cents)?;

    // Stream must be live so we don't accept money for nothing.
    let stream_row: Option<(Uuid, bool, String)> = sqlx::query_as(
        "SELECT user_id, is_live, title FROM streams WHERE id = $1",
    )
    .bind(body.stream_id)
    .fetch_optional(&pool)
    .await?;
    let Some((streamer_id, is_live, stream_title)) = stream_row else {
        return Err(AppError::BadRequest("stream not found".to_string()));
    };
    if !is_live {
        return Err(AppError::BadRequest("stream is not live".to_string()));
    }

    // Streamer must have completed Stripe onboarding so we have a
    // destination account to transfer the funds to.
    let owner_row: Option<(Option<String>, bool, String)> = sqlx::query_as(
        "SELECT stripe_account_id, stripe_onboarding_complete, username \
         FROM users WHERE id = $1",
    )
    .bind(streamer_id)
    .fetch_optional(&pool)
    .await?;
    let Some((account_id_opt, onboarded, username)) = owner_row else {
        return Err(AppError::BadRequest("streamer not found".to_string()));
    };
    if !onboarded {
        return Err(AppError::BadRequest(
            "this streamer has not enabled donations yet".to_string(),
        ));
    }
    let account_id_str = account_id_opt.filter(|s| !s.is_empty()).ok_or_else(|| {
        AppError::BadRequest("this streamer has not enabled donations yet".to_string())
    })?;
    let destination_account = stripe::AccountId::from_str(&account_id_str)
        .map_err(|_| AppError::Internal("invalid stored stripe account id".to_string()))?;

    // SECURITY: A sound is the streamer's property; rejecting any
    // sound_id that doesn't belong to this stream's owner stops a
    // donor from triggering arbitrary other streamers' audio on this
    // stream's overlay.
    if let Some(sound_id) = body.sound_id {
        let sound_owner: Option<(Uuid,)> =
            sqlx::query_as("SELECT streamer_id FROM donation_sounds WHERE id = $1")
                .bind(sound_id)
                .fetch_optional(&pool)
                .await?;
        match sound_owner {
            Some((owner,)) if owner == streamer_id => {}
            _ => {
                return Err(AppError::Forbidden(
                    "sound does not belong to this streamer".to_string(),
                ));
            }
        }
    }

    let application_fee_amount = application_fee_amount(body.amount_cents);

    let success_url = format!(
        "{}/watch/{}?donation=success",
        cfg.frontend_url, body.stream_id
    );
    let cancel_url = format!(
        "{}/watch/{}?donation=cancelled",
        cfg.frontend_url, body.stream_id
    );

    let product_name = format!("Donation to {username}");
    let product_data = CreateCheckoutSessionLineItemsPriceDataProductData {
        name: product_name,
        description: Some(format!("Live stream: {stream_title}")),
        ..Default::default()
    };

    let price_data = CreateCheckoutSessionLineItemsPriceData {
        currency: Currency::USD,
        unit_amount: Some(body.amount_cents),
        product_data: Some(product_data),
        ..Default::default()
    };

    let line_item = CreateCheckoutSessionLineItems {
        price_data: Some(price_data),
        quantity: Some(1),
        ..Default::default()
    };

    let transfer_data = CreateCheckoutSessionPaymentIntentDataTransferData {
        destination: destination_account.to_string(),
        amount: None,
    };

    let payment_intent_data = CreateCheckoutSessionPaymentIntentData {
        application_fee_amount: Some(application_fee_amount),
        transfer_data: Some(transfer_data),
        ..Default::default()
    };

    let mut metadata: HashMap<String, String> = HashMap::new();
    metadata.insert("stream_id".to_string(), body.stream_id.to_string());
    metadata.insert("donor_name".to_string(), donor_name.clone());
    metadata.insert(
        "sound_id".to_string(),
        body.sound_id.map(|s| s.to_string()).unwrap_or_default(),
    );

    let mut params = CreateCheckoutSession::new();
    params.mode = Some(CheckoutSessionMode::Payment);
    params.success_url = Some(&success_url);
    params.cancel_url = Some(&cancel_url);
    params.line_items = Some(vec![line_item]);
    params.payment_intent_data = Some(payment_intent_data);
    params.metadata = Some(metadata);

    let client = Client::new(cfg.stripe_secret_key.clone());
    let session = CheckoutSession::create(&client, params).await.map_err(|e| {
        tracing::error!("stripe checkout session create failed: {e:?}");
        AppError::Internal("failed to create checkout session".to_string())
    })?;

    let checkout_url = session.url.clone().ok_or_else(|| {
        AppError::Internal("stripe did not return a checkout url".to_string())
    })?;

    sqlx::query(
        "INSERT INTO donations \
         (stream_id, donor_name, amount_cents, sound_id, stripe_session_id, status) \
         VALUES ($1, $2, $3, $4, $5, 'pending')",
    )
    .bind(body.stream_id)
    .bind(&donor_name)
    .bind(body.amount_cents)
    .bind(body.sound_id)
    .bind(session.id.as_str())
    .execute(&pool)
    .await?;

    Ok(Json(json!({ "checkout_url": checkout_url })))
}

#[derive(Debug, Deserialize)]
struct HistoryQuery {
    streamer_id: Uuid,
}

/// `GET /api/donations?streamer_id=…` — last 50 completed donations
/// across every stream the caller owns. Authenticated streamer only:
/// the path's `streamer_id` must match the JWT subject so a streamer
/// can't peek at someone else's earnings.
async fn history(
    State((pool, _cfg)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Query(q): Query<HistoryQuery>,
) -> Result<Json<Value>, AppError> {
    if auth.user_id != q.streamer_id {
        // SECURITY: 403 (not 401) — the caller IS authenticated, they
        // just aren't allowed to read another streamer's donation history.
        return Err(AppError::Forbidden(
            "cannot view another streamer's donations".to_string(),
        ));
    }

    let rows: Vec<(Uuid, Uuid, String, String, i32, chrono::DateTime<chrono::Utc>)> =
        sqlx::query_as(
            "SELECT d.id, d.stream_id, s.title, d.donor_name, d.amount_cents, d.created_at \
             FROM donations d JOIN streams s ON s.id = d.stream_id \
             WHERE s.user_id = $1 AND d.status = 'completed' \
             ORDER BY d.created_at DESC LIMIT 50",
        )
        .bind(auth.user_id)
        .fetch_all(&pool)
        .await?;

    let payload: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, stream_id, stream_title, donor_name, amount_cents, created_at)| {
                json!({
                    "id": id,
                    "stream_id": stream_id,
                    "stream_title": stream_title,
                    "donor_name": donor_name,
                    "amount_cents": amount_cents,
                    "created_at": created_at,
                })
            },
        )
        .collect();

    Ok(Json(Value::Array(payload)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_bad_request<T: std::fmt::Debug>(result: Result<T, AppError>) -> String {
        match result {
            Err(AppError::BadRequest(msg)) => msg,
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    #[test]
    fn checkout_minimum_amount_accepted() {
        let name = validate_checkout_input("alice", MIN_DONATION_CENTS).unwrap();
        assert_eq!(name, "alice");
    }

    #[test]
    fn checkout_below_minimum_rejected() {
        let msg = assert_bad_request(validate_checkout_input("alice", MIN_DONATION_CENTS - 1));
        assert!(msg.contains("minimum"));
    }

    #[test]
    fn checkout_maximum_amount_accepted() {
        let name = validate_checkout_input("alice", MAX_DONATION_CENTS).unwrap();
        assert_eq!(name, "alice");
    }

    #[test]
    fn checkout_above_maximum_rejected() {
        let msg = assert_bad_request(validate_checkout_input("alice", MAX_DONATION_CENTS + 1));
        assert!(msg.contains("maximum"));
    }

    #[test]
    fn checkout_empty_donor_name_rejected() {
        let msg = assert_bad_request(validate_checkout_input("   ", 500));
        assert!(msg.contains("donor_name"));
    }

    #[test]
    fn checkout_donor_name_trimmed() {
        let name = validate_checkout_input("  bob  ", 500).unwrap();
        assert_eq!(name, "bob");
    }

    #[test]
    fn checkout_donor_name_length_capped() {
        let too_long: String = "a".repeat(MAX_DONOR_NAME_LEN + 1);
        let msg = assert_bad_request(validate_checkout_input(&too_long, 500));
        assert!(msg.contains("characters"));
    }

    #[test]
    fn checkout_donor_name_at_cap_accepted() {
        let exact: String = "a".repeat(MAX_DONOR_NAME_LEN);
        assert_eq!(
            validate_checkout_input(&exact, 500).unwrap().chars().count(),
            MAX_DONOR_NAME_LEN
        );
    }

    /// SECURITY-CRITICAL: pin the 85/15 split. If this ever drifts, the
    /// platform will silently start charging the wrong fee.
    #[test]
    fn application_fee_is_15_percent_floored() {
        assert_eq!(application_fee_amount(100), 15);
        assert_eq!(application_fee_amount(500), 75);
        assert_eq!(application_fee_amount(1_000), 150);
        assert_eq!(application_fee_amount(1_000_000), 150_000);
        // Floor (not round): 15% of 199 = 29.85 → 29
        assert_eq!(application_fee_amount(199), 29);
    }
}
