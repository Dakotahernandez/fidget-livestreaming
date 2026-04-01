use axum::{
    extract::State,
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::config::Config;
use crate::errors::AppError;
use crate::middleware::auth::create_token;
use crate::models::user::{self, CreateUser, LoginUser};

/// Build the auth sub-router.
pub fn router() -> Router<(PgPool, Config)> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
}

/// `POST /api/auth/register`
async fn register(
    State((pool, config)): State<(PgPool, Config)>,
    Json(body): Json<CreateUser>,
) -> Result<Json<Value>, AppError> {
    if body.username.trim().is_empty() || body.password.trim().is_empty() {
        return Err(AppError::BadRequest("username and password are required".to_string()));
    }

    let hash = bcrypt::hash(&body.password, bcrypt::DEFAULT_COST)?;
    let new_user = user::create_user(&pool, &body.username, &hash).await?;
    let token = create_token(new_user.id, &config.jwt_secret)?;

    Ok(Json(json!({
        "token": token,
        "user_id": new_user.id,
        "username": new_user.username,
    })))
}

/// `POST /api/auth/login`
async fn login(
    State((pool, config)): State<(PgPool, Config)>,
    Json(body): Json<LoginUser>,
) -> Result<Json<Value>, AppError> {
    let stored = user::find_by_username(&pool, &body.username)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid credentials".to_string()))?;

    let valid = bcrypt::verify(&body.password, &stored.password_hash)
        .map_err(|_| AppError::Unauthorized("invalid credentials".to_string()))?;

    if !valid {
        return Err(AppError::Unauthorized("invalid credentials".to_string()));
    }

    let token = create_token(stored.id, &config.jwt_secret)?;

    Ok(Json(json!({
        "token": token,
        "user_id": stored.id,
        "username": stored.username,
    })))
}
