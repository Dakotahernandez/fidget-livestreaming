use axum::{
    extract::Request,
    http::header::AUTHORIZATION,
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::AppError;

/// JWT lifetime in seconds (24 hours).
const TOKEN_EXPIRY_SECS: i64 = 86_400;

/// Claims encoded in every JWT.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: Uuid,
    pub exp: i64,
}

/// Authenticated user id — extracted by the middleware and placed in
/// request extensions so handlers can retrieve it.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
}

/// Create a signed JWT for the given user id.
pub fn create_token(user_id: Uuid, secret: &str) -> Result<String, AppError> {
    let exp = chrono::Utc::now().timestamp() + TOKEN_EXPIRY_SECS;
    let claims = Claims { sub: user_id, exp };

    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("token creation failed: {e}")))
}

/// Axum middleware: validates the `Authorization: Bearer <token>` header
/// and injects `AuthUser` into request extensions.
pub async fn require_auth(
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let secret = request
        .extensions()
        .get::<String>()
        .ok_or_else(|| AppError::Internal("JWT secret not configured".to_string()))?
        .clone();

    let header_value = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("missing authorization header".to_string()))?;

    let token = header_value
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::Unauthorized("invalid authorization format".to_string()))?;

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("invalid or expired token".to_string()))?;

    let mut request = request;
    request
        .extensions_mut()
        .insert(AuthUser { user_id: token_data.claims.sub });

    Ok(next.run(request).await)
}
