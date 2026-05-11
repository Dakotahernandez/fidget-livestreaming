use axum::{
    extract::State,
    middleware as axum_mw,
    routing::post,
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;

use crate::config::Config;
use crate::errors::AppError;
use crate::middleware::auth::{require_auth, AuthUser};
use crate::models::{role_application, user};

/// Build the role application sub-router.
pub fn router() -> Router<(PgPool, Config)> {
    Router::new()
    .route("/", post(apply_for_role).get(list_my_applications))
        .layer(axum_mw::from_fn(require_auth))
}

#[derive(Debug, Deserialize)]
struct ApplyRoleRequest {
    reason: Option<String>,
    target_role: Option<String>,
}

/// `POST /api/role-applications` — submit a role request.
async fn apply_for_role(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Json(body): Json<ApplyRoleRequest>,
) -> Result<Json<Value>, AppError> {
    let current_user = user::find_by_id(&pool, auth.user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;

    if current_user.role != "viewer" {
        return Err(AppError::BadRequest(
            "only viewers can submit role applications".to_string(),
        ));
    }

    let requested_role = body
        .target_role
        .unwrap_or_else(|| "streamer".to_string())
        .trim()
        .to_string();

    if requested_role != "streamer" && requested_role != "admin" {
        return Err(AppError::BadRequest(
            "target role must be streamer or admin".to_string(),
        ));
    }

    let reason = body
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());

    let app = role_application::create_application(&pool, auth.user_id, &requested_role, reason).await?;

    Ok(Json(serde_json::json!({
        "id": app.id,
        "user_id": app.user_id,
        "target_role": app.target_role,
        "reason": app.reason,
        "status": app.status,
        "created_at": app.created_at,
        "reviewed_at": app.reviewed_at,
        "reviewed_by": app.reviewed_by,
    })))
}

/// `GET /api/role-applications` — list current user's applications.
async fn list_my_applications(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<Value>>, AppError> {
    let apps = role_application::list_user_applications(&pool, auth.user_id).await?;

    let payload: Vec<Value> = apps
        .into_iter()
        .map(|app| {
            serde_json::json!({
                "id": app.id,
                "user_id": app.user_id,
                "target_role": app.target_role,
                "reason": app.reason,
                "status": app.status,
                "created_at": app.created_at,
                "reviewed_at": app.reviewed_at,
                "reviewed_by": app.reviewed_by,
            })
        })
        .collect();

    Ok(Json(payload))
}
