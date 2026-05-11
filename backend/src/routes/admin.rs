use axum::{
    extract::{Path, State},
    middleware as axum_mw,
    routing::{get, patch},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::errors::AppError;
use crate::middleware::auth::{require_auth, AuthUser};
use crate::models::user;

//Builds the /api/admin/* sub-router. Every route here requires a valid JWT.
pub fn router() -> Router<(PgPool, Config)> {
    Router::new()
        .route("/users", get(list_users))
        .route("/users/:id/role", patch(update_user_role))
        .layer(axum_mw::from_fn(require_auth))
}

//Body for PATCH /users/:id/role.
#[derive(Debug, Deserialize)]
struct UpdateRoleRequest {
    role: String,
}

//Allowed role values.
fn is_valid_role(role: &str) -> bool {
    role == "admin" || role == "streamer" || role == "viewer"
}

/// Look up the caller and 403 if they aren't an admin.
async fn require_admin(pool: &PgPool, auth: &AuthUser) -> Result<(), AppError> {
    let current_user = user::find_by_id(pool, auth.user_id)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".to_string()))?;

    if current_user.role != "admin" {
        return Err(AppError::Forbidden(
            "admin permission required".to_string(),
        ));
    }

    Ok(())
}

/// `GET /api/admin/users` — return every user. Admin-only.
async fn list_users(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<Value>>, AppError> {
    require_admin(&pool, &auth).await?;

    let users = user::list_users(&pool).await?;

    let payload: Vec<Value> = users
        .into_iter()
        .map(|u| {
            serde_json::json!({
                "id": u.id,
                "username": u.username,
                "email": u.email,
                "role": u.role,
                "created_at": u.created_at,
            })
        })
        .collect();

    Ok(Json(payload))
}

/// `PATCH /api/admin/users/:id/role` — change a user's role. Admin-only.
async fn update_user_role(
    State((pool, _config)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRoleRequest>,
) -> Result<Json<Value>, AppError> {
    require_admin(&pool, &auth).await?;

    let role = body.role.trim();

    if !is_valid_role(role) {
        return Err(AppError::BadRequest(
            "role must be admin, streamer, or viewer".to_string(),
        ));
    }

    let updated = user::update_role(&pool, id, role).await?;

    Ok(Json(serde_json::json!({
        "id": updated.id,
        "username": updated.username,
        "email": updated.email,
        "role": updated.role,
        "created_at": updated.created_at,
    })))
}