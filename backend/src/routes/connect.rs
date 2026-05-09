//! Stripe Connect onboarding for streamers (Express accounts +
//! Account Links). Streamers must finish onboarding (Stripe sets
//! `charges_enabled = true`) before the donations checkout endpoint
//! will accept payments routed to their account.

use axum::{
    extract::State,
    middleware as axum_mw,
    response::Redirect,
    routing::{get, post},
    Extension, Json, Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::str::FromStr;
use stripe::{
    Account, AccountLink, AccountLinkType, AccountType, Client, CreateAccount,
    CreateAccountLink,
};

use crate::config::Config;
use crate::errors::AppError;
use crate::middleware::auth::{require_auth, AuthUser};

/// Build the Stripe Connect sub-router.
pub fn router() -> Router<(PgPool, Config)> {
    let public = Router::new().route("/callback", get(callback));
    let protected = Router::new()
        .route("/onboard", post(onboard))
        .route("/status", get(status))
        .layer(axum_mw::from_fn(require_auth));
    public.merge(protected)
}

/// `POST /api/connect/onboard` — create (or reuse) the streamer's
/// Express account and return a fresh Account Link the client can
/// redirect to.
async fn onboard(
    State((pool, cfg)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Value>, AppError> {
    let row: (Option<String>, String) = sqlx::query_as(
        "SELECT stripe_account_id, email FROM users WHERE id = $1",
    )
    .bind(auth.user_id)
    .fetch_one(&pool)
    .await?;

    let (existing_account_id, email) = row;
    let client = Client::new(cfg.stripe_secret_key.clone());

    // Reuse an existing Stripe account if the streamer started onboarding
    // before but never finished, otherwise create a fresh Express account.
    let account_id = match existing_account_id {
        Some(id) if !id.is_empty() => stripe::AccountId::from_str(&id)
            .map_err(|_| AppError::Internal("invalid stored stripe account id".to_string()))?,
        _ => {
            let mut params = CreateAccount::new();
            params.type_ = Some(AccountType::Express);
            params.email = Some(&email);
            let account = Account::create(&client, params).await.map_err(stripe_err)?;
            sqlx::query("UPDATE users SET stripe_account_id = $1 WHERE id = $2")
                .bind(account.id.to_string())
                .bind(auth.user_id)
                .execute(&pool)
                .await?;
            account.id
        }
    };

    let return_url = format!("{}/dashboard?connect=success", cfg.frontend_url);
    let refresh_url = format!("{}/dashboard?connect=refresh", cfg.frontend_url);

    let mut link_params = CreateAccountLink::new(account_id.clone(), AccountLinkType::AccountOnboarding);
    link_params.return_url = Some(&return_url);
    link_params.refresh_url = Some(&refresh_url);

    let link = AccountLink::create(&client, link_params)
        .await
        .map_err(stripe_err)?;

    Ok(Json(json!({ "url": link.url })))
}

/// `GET /api/connect/status` — re-fetch the streamer's account from
/// Stripe so we know whether `charges_enabled` is finally true and can
/// flip the local cached flag.
async fn status(
    State((pool, cfg)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Value>, AppError> {
    let row: (Option<String>, bool) = sqlx::query_as(
        "SELECT stripe_account_id, stripe_onboarding_complete FROM users WHERE id = $1",
    )
    .bind(auth.user_id)
    .fetch_one(&pool)
    .await?;

    let (account_id, mut onboarded) = row;

    if let Some(id) = account_id.as_deref().filter(|s| !s.is_empty()) {
        let parsed = stripe::AccountId::from_str(id)
            .map_err(|_| AppError::Internal("invalid stored stripe account id".to_string()))?;
        let client = Client::new(cfg.stripe_secret_key.clone());
        match Account::retrieve(&client, &parsed, &[]).await {
            Ok(account) => {
                let charges_enabled = account.charges_enabled.unwrap_or(false);
                if charges_enabled != onboarded {
                    sqlx::query(
                        "UPDATE users SET stripe_onboarding_complete = $1 WHERE id = $2",
                    )
                    .bind(charges_enabled)
                    .bind(auth.user_id)
                    .execute(&pool)
                    .await?;
                    onboarded = charges_enabled;
                }
            }
            Err(err) => {
                tracing::error!("stripe account retrieve failed: {err:?}");
            }
        }
    }

    Ok(Json(json!({
        "onboarded": onboarded,
        "account_id": account_id,
    })))
}

/// `GET /api/connect/callback` — redirect helper preserved for spec
/// compatibility. The actual flag is flipped by `/api/connect/status`,
/// which the dashboard re-fetches on `?connect=success`.
async fn callback(State((_pool, cfg)): State<(PgPool, Config)>) -> Redirect {
    Redirect::to(&format!("{}/dashboard?connect=success", cfg.frontend_url))
}

/// Map any Stripe SDK error to an internal 500 — donors and streamers
/// only need to know that *something* upstream failed; the detail is
/// logged for operators.
fn stripe_err<E: std::fmt::Debug>(err: E) -> AppError {
    tracing::error!("stripe error: {err:?}");
    AppError::Internal("stripe request failed".to_string())
}
