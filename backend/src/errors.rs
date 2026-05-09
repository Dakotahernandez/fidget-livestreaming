use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Unified error type returned by route handlers.
#[derive(Debug)]
pub enum AppError {
    NotFound(String),
    Unauthorized(String),
    // SECURITY: distinct from Unauthorized so handlers can return 403 when
    // the caller is authenticated but not allowed to touch a specific
    // resource (e.g. another streamer's sound). Returning 404 for
    // ownership failures hides the existence-vs-permission distinction
    Forbidden(String),
    BadRequest(String),
    // SECURITY: dedicated 415 for upload content-type rejections so we
    // never silently coerce non-audio bytes to disk.
    UnsupportedMediaType(String),
    Internal(String),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(msg) => write!(f, "Not found: {msg}"),
            Self::Unauthorized(msg) => write!(f, "Unauthorized: {msg}"),
            Self::Forbidden(msg) => write!(f, "Forbidden: {msg}"),
            Self::BadRequest(msg) => write!(f, "Bad request: {msg}"),
            Self::UnsupportedMediaType(msg) => write!(f, "Unsupported media type: {msg}"),
            Self::Internal(msg) => write!(f, "Internal error: {msg}"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.clone()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Self::UnsupportedMediaType(msg) => {
                (StatusCode::UNSUPPORTED_MEDIA_TYPE, msg.clone())
            }
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };

        let body = Json(json!({ "error": message }));
        (status, body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!("Database error: {err:?}");
        Self::Internal("database error".to_string())
    }
}

impl From<bcrypt::BcryptError> for AppError {
    fn from(err: bcrypt::BcryptError) -> Self {
        tracing::error!("Bcrypt error: {err:?}");
        Self::Internal("password hashing error".to_string())
    }
}
