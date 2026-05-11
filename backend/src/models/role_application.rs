use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;

const STATUS_PENDING: &str = "pending";
const STATUS_APPROVED: &str = "approved";
const STATUS_DENIED: &str = "denied";

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RoleApplication {
    pub id: Uuid,
    pub user_id: Uuid,
    pub target_role: String,
    pub reason: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub reviewed_by: Option<Uuid>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PendingRoleApplication {
    pub id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub email: String,
    pub target_role: String,
    pub reason: Option<String>,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

pub async fn create_application(
    pool: &PgPool,
    user_id: Uuid,
    target_role: &str,
    reason: Option<&str>,
) -> Result<RoleApplication, AppError> {
    let existing = sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT id
        FROM role_applications
        WHERE user_id = $1 AND status = $2
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(STATUS_PENDING)
    .fetch_optional(pool)
    .await?;

    if existing.is_some() {
        return Err(AppError::BadRequest(
            "you already have a pending role application".to_string(),
        ));
    }

    let app = sqlx::query_as::<_, RoleApplication>(
        r#"
        INSERT INTO role_applications (user_id, target_role, reason)
        VALUES ($1, $2, $3)
        RETURNING id, user_id, target_role, reason, status, created_at, reviewed_at, reviewed_by
        "#,
    )
    .bind(user_id)
    .bind(target_role)
    .bind(reason)
    .fetch_one(pool)
    .await?;

    Ok(app)
}

pub async fn list_user_applications(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Vec<RoleApplication>, AppError> {
    let apps = sqlx::query_as::<_, RoleApplication>(
        r#"
        SELECT id, user_id, target_role, reason, status, created_at, reviewed_at, reviewed_by
        FROM role_applications
        WHERE user_id = $1
        ORDER BY created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(apps)
}

pub async fn list_pending_applications(
    pool: &PgPool,
) -> Result<Vec<PendingRoleApplication>, AppError> {
    let apps = sqlx::query_as::<_, PendingRoleApplication>(
        r#"
        SELECT
            role_applications.id,
            role_applications.user_id,
            users.username,
            users.email,
            role_applications.target_role,
            role_applications.reason,
            role_applications.status,
            role_applications.created_at
        FROM role_applications
        JOIN users ON users.id = role_applications.user_id
        WHERE role_applications.status = $1
        ORDER BY role_applications.created_at ASC
        "#,
    )
    .bind(STATUS_PENDING)
    .fetch_all(pool)
    .await?;

    Ok(apps)
}

pub async fn review_application(
    pool: &PgPool,
    application_id: Uuid,
    reviewer_id: Uuid,
    approve: bool,
) -> Result<RoleApplication, AppError> {
    let next_status = if approve {
        STATUS_APPROVED
    } else {
        STATUS_DENIED
    };

    let mut tx = pool.begin().await?;

    let app = sqlx::query_as::<_, RoleApplication>(
        r#"
        UPDATE role_applications
        SET status = $1, reviewed_at = NOW(), reviewed_by = $2
        WHERE id = $3 AND status = $4
        RETURNING id, user_id, target_role, reason, status, created_at, reviewed_at, reviewed_by
        "#,
    )
    .bind(next_status)
    .bind(reviewer_id)
    .bind(application_id)
    .bind(STATUS_PENDING)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| AppError::BadRequest("application not found or already reviewed".to_string()))?;

    if approve && (app.target_role == "streamer" || app.target_role == "admin") {
        sqlx::query(
            r#"
            UPDATE users
            SET role = $1
            WHERE id = $2
            "#,
        )
        .bind(&app.target_role)
        .bind(app.user_id)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(app)
}
