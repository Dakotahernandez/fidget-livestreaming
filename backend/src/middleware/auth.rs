use axum::{
    extract::Request,
    http::{header::AUTHORIZATION, HeaderMap},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::AppError;

//JWT lifetime - 24 hours.
const TOKEN_EXPIRY_SECS: i64 = 86_400;

/// Chat access token lifetime in seconds (5 minutes).
pub const CHAT_TOKEN_EXPIRY_SECS: i64 = 300;

/// Claims encoded in every JWT.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: Uuid,  //user id
    pub exp: i64,   //unix-seconds expiry
}

/// Authenticated user id. The middleware decodes the JWT and stuffs this into
/// request extensions so route handlers can pull it back out.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
}

/// Claims for short-lived stream-scoped chat access tokens.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatAccessClaims {
    pub sub: Uuid,
    pub stream_id: Uuid,
    pub exp: i64,
}

fn bearer_token_from_headers(headers: &HeaderMap) -> Result<Option<String>, AppError> {
    let Some(header_value) = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    else {
        return Ok(None);
    };

    let token = header_value
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::Unauthorized("invalid authorization format".to_string()))?;

    Ok(Some(token.to_string()))
}

/// Decode a regular session JWT and return its claims.
pub fn decode_session_token(token: &str, secret: &str) -> Result<Claims, AppError> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("invalid or expired token".to_string()))?;

    Ok(token_data.claims)
}

/// Decode a short-lived chat token and return its claims.
pub fn decode_chat_access_token(token: &str, secret: &str) -> Result<ChatAccessClaims, AppError> {
    let token_data = decode::<ChatAccessClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("invalid or expired chat token".to_string()))?;

    Ok(token_data.claims)
}

/// Extract an authenticated user from an Authorization header when present.
///
/// Returns `Ok(None)` when the header is missing.
pub fn extract_optional_auth_user(
    headers: &HeaderMap,
    secret: &str,
) -> Result<Option<AuthUser>, AppError> {
    let Some(token) = bearer_token_from_headers(headers)? else {
        return Ok(None);
    };

    let claims = decode_session_token(&token, secret)?;
    Ok(Some(AuthUser { user_id: claims.sub }))
}

/// Build a signed JWT for the given user id.
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

/// Create a short-lived stream-scoped signed token for chat access.
pub fn create_chat_access_token(
    user_id: Uuid,
    stream_id: Uuid,
    secret: &str,
) -> Result<String, AppError> {
    let exp = chrono::Utc::now().timestamp() + CHAT_TOKEN_EXPIRY_SECS;
    let claims = ChatAccessClaims {
        sub: user_id,
        stream_id,
        exp,
    };

    jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AppError::Internal(format!("chat token creation failed: {e}")))
}

/// Middleware. Validates Authorization and injects an
/// 'AuthUser' into request extensions. Returns 401 on any failure.
pub async fn require_auth(
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    //Pull the JWT secret out of the request's extensions (planted by main.rs).
    let secret = request
        .extensions()
        .get::<String>()
        .ok_or_else(|| AppError::Internal("JWT secret not configured".to_string()))?
        .clone();

    let auth_user = extract_optional_auth_user(request.headers(), &secret)?
        .ok_or_else(|| AppError::Unauthorized("missing authorization header".to_string()))?
;
    let mut request = request;
    request.extensions_mut().insert(auth_user);

    Ok(next.run(request).await)
}
