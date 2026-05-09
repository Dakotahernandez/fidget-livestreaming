//! Per-streamer donation sound library. Audio files are stored on disk
//! under `{upload_dir}/{streamer_id}/{uuid}.{ext}` and surfaced through
//! the `/uploads` ServeDir. Sounds are referenced from `donations.sound_id`
//! so completed donations can resolve their alert audio at webhook time.

use axum::{
    extract::{Multipart, Path, Query, State},
    middleware as axum_mw,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::path::PathBuf;
use uuid::Uuid;

use crate::config::Config;
use crate::errors::AppError;
use crate::middleware::auth::{require_auth, AuthUser};

/// Build the sounds sub-router. Listing is public (donors need it on the
/// watch page); upload + delete require streamer auth.
pub fn router() -> Router<(PgPool, Config)> {
    let public = Router::new().route("/", get(list_sounds));
    let protected = Router::new()
        .route("/", post(upload_sound))
        .route("/:id", delete(delete_sound))
        .layer(axum_mw::from_fn(require_auth));
    public.merge(protected)
}

/// SECURITY: Hard cap on the user-supplied display name. The name is
/// echoed back into the donor sound picker (and into broadcast SSE
/// payloads), so a megabyte name would balloon every list response.
pub const MAX_DISPLAY_NAME_LEN: usize = 80;

/// SECURITY: Strict allowlist of audio MIME types the upload endpoint
/// accepts. Anything not on this list is rejected with `415 Unsupported
/// Media Type` before a single byte is written to disk. We do NOT trust
/// the file extension alone — the browser-supplied content-type must
/// also be on the list.
pub const ALLOWED_AUDIO_MIME: &[&str] = &[
    "audio/mpeg",
    "audio/mp3",
    "audio/ogg",
    "audio/wav",
    "audio/x-wav",
    "audio/wave",
    "audio/webm",
    "audio/aac",
];

/// Pure helper so the test suite can pin the allowlist behavior without
/// needing a multipart body.
pub fn is_allowed_audio_mime(content_type: &str) -> bool {
    // Strip any `; charset=...` parameters before comparing.
    let base = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();
    ALLOWED_AUDIO_MIME.iter().any(|m| *m == base)
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    stream_id: Uuid,
}

/// `GET /api/sounds?stream_id=…` — list every sound owned by the user
/// who owns the stream. Donors call this to populate the sound picker.
async fn list_sounds(
    State((pool, _cfg)): State<(PgPool, Config)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, AppError> {
    let owner_id: Option<(Uuid,)> = sqlx::query_as("SELECT user_id FROM streams WHERE id = $1")
        .bind(q.stream_id)
        .fetch_optional(&pool)
        .await?;
    let Some((streamer_id,)) = owner_id else {
        return Err(AppError::NotFound("stream not found".to_string()));
    };

    let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, display_name, file_path FROM donation_sounds \
         WHERE streamer_id = $1 ORDER BY created_at ASC",
    )
    .bind(streamer_id)
    .fetch_all(&pool)
    .await?;

    let payload: Vec<Value> = rows
        .into_iter()
        .map(|(id, display_name, file_path)| {
            json!({
                "id": id,
                "display_name": display_name,
                "url": file_path_to_url(&file_path),
            })
        })
        .collect();

    Ok(Json(Value::Array(payload)))
}

/// `POST /api/sounds` — multipart upload (`display_name` text field +
/// audio `file` field). Enforces the 2 MB and 10-sound caps.
async fn upload_sound(
    State((pool, cfg)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM donation_sounds WHERE streamer_id = $1")
            .bind(auth.user_id)
            .fetch_one(&pool)
            .await?;
    if count.0 >= cfg.sounds_per_streamer_max {
        return Err(AppError::BadRequest(format!(
            "sound limit reached ({} max)",
            cfg.sounds_per_streamer_max
        )));
    }

    let mut display_name: Option<String> = None;
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_ext: String = "bin".to_string();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::BadRequest(format!("invalid multipart upload: {e}")))?
    {
        match field.name().unwrap_or_default() {
            "display_name" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("invalid display_name: {e}")))?;
                display_name = Some(text.trim().to_string());
            }
            "file" => {
                let original_name = field.file_name().unwrap_or_default().to_string();
                let content_type = field.content_type().unwrap_or_default().to_string();
                // SECURITY: Strict allowlist check returns 415 (not 400)
                // before we read a single byte off the wire. The browser-
                // supplied content-type is the *only* thing we trust here;
                // the file extension on `original_name` is treated as
                // untrusted user input below.
                if !is_allowed_audio_mime(&content_type) {
                    return Err(AppError::UnsupportedMediaType(format!(
                        "unsupported audio content type: {content_type}"
                    )));
                }
                file_ext = sanitize_extension(&original_name, &content_type);
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| AppError::BadRequest(format!("upload read failed: {e}")))?;
                // SECURITY: Reject anything over the configured cap.
                // axum's `DefaultBodyLimit` (set on the route layer in
                // main.rs) is the first line of defense; this check is
                // belt-and-suspenders in case the limit layer is ever
                // re-ordered.
                if bytes.len() as u64 > cfg.upload_max_bytes {
                    return Err(AppError::BadRequest(format!(
                        "file too large ({} bytes max)",
                        cfg.upload_max_bytes
                    )));
                }
                file_bytes = Some(bytes.to_vec());
            }
            _ => { /* ignore unknown fields for forward-compat */ }
        }
    }

    let display_name = display_name
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::BadRequest("display_name is required".to_string()))?;
    // SECURITY: enforce the display-name cap server-side so a hostile
    // streamer can't ship megabyte names that bloat every list response
    // or burst the SSE payload.
    if display_name.chars().count() > MAX_DISPLAY_NAME_LEN {
        return Err(AppError::BadRequest(format!(
            "display_name must be {MAX_DISPLAY_NAME_LEN} characters or fewer"
        )));
    }
    let file_bytes = file_bytes
        .ok_or_else(|| AppError::BadRequest("audio file is required".to_string()))?;

    // SECURITY: The on-disk filename is *always* a freshly minted UUID
    // plus a sanitized extension. The donor's original filename is never
    // joined into the path, so a name like `../../etc/passwd` cannot
    // escape the per-streamer subdirectory.
    let sound_id = Uuid::new_v4();
    let mut path = PathBuf::from(&cfg.upload_dir);
    path.push(auth.user_id.to_string());
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|e| AppError::Internal(format!("failed to create upload dir: {e}")))?;
    path.push(format!("{}.{}", sound_id, file_ext));

    // SECURITY: Defense-in-depth path-traversal check. Even though the
    // filename is a UUID we generate, we still confirm the resolved path
    // sits inside `upload_dir`. If the resolved path ever climbs above
    // the configured directory we refuse to write.
    if !path_is_inside(&path, std::path::Path::new(&cfg.upload_dir)) {
        return Err(AppError::BadRequest(
            "invalid upload path".to_string(),
        ));
    }

    tokio::fs::write(&path, &file_bytes)
        .await
        .map_err(|e| AppError::Internal(format!("failed to write upload: {e}")))?;

    let stored_path = path.to_string_lossy().to_string();

    sqlx::query(
        "INSERT INTO donation_sounds (id, streamer_id, display_name, file_path) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(sound_id)
    .bind(auth.user_id)
    .bind(&display_name)
    .bind(&stored_path)
    .execute(&pool)
    .await?;

    Ok(Json(json!({
        "id": sound_id,
        "display_name": display_name,
        "url": file_path_to_url(&stored_path),
    })))
}

/// `DELETE /api/sounds/:id` — owner-only. Removes the row + the file on
/// disk (best-effort; a missing file just logs a warning).
async fn delete_sound(
    State((pool, cfg)): State<(PgPool, Config)>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, AppError> {
    // SECURITY: Look up the sound *without* the owner filter first so we
    // can distinguish "doesn't exist" (404) from "exists but you don't
    // own it" (403). Returning 404 for both would still be safe, but the
    // spec requires 403 for the cross-streamer attack so the test suite
    // can pin the behavior.
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT streamer_id, file_path FROM donation_sounds WHERE id = $1")
            .bind(id)
            .fetch_optional(&pool)
            .await?;
    let Some((streamer_id, file_path)) = row else {
        return Err(AppError::NotFound("sound not found".to_string()));
    };
    if streamer_id != auth.user_id {
        return Err(AppError::Forbidden(
            "you do not own this sound".to_string(),
        ));
    }

    sqlx::query("DELETE FROM donation_sounds WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await?;

    if let Err(err) = tokio::fs::remove_file(&file_path).await {
        tracing::warn!(
            "failed to remove sound file {file_path} (db row already gone): {err}"
        );
    }

    // `cfg` is intentionally unused here — kept in the signature so
    // future cleanup can reach into upload limits without re-threading
    // state.
    let _ = &cfg;

    Ok(Json(json!({ "deleted": id })))
}

/// Resolve a `donation_sounds.file_path` to the public `/uploads/...`
/// URL exposed by the `ServeDir` mount in `main.rs`. The stored path is
/// always `{upload_dir}/{streamer_id}/{uuid}.{ext}`, so we just keep
/// the trailing `{streamer_id}/{file}` segment.
pub fn file_path_to_url(file_path: &str) -> String {
    let parts: Vec<&str> = file_path.rsplit('/').take(2).collect();
    if parts.len() == 2 {
        // rsplit gives [file, streamer_id]; reverse for the URL.
        return format!("/uploads/{}/{}", parts[1], parts[0]);
    }
    format!("/uploads/{file_path}")
}

/// Pick a safe lowercase extension based on the original filename or the
/// declared MIME type. Falls back to `bin` so we never write a
/// path-traversal-friendly value to disk.
pub(crate) fn sanitize_extension(original_name: &str, content_type: &str) -> String {
    let from_name = std::path::Path::new(original_name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    if let Some(ext) = from_name {
        if ext.chars().all(|c| c.is_ascii_alphanumeric()) && (1..=8).contains(&ext.len()) {
            return ext;
        }
    }
    match content_type {
        "audio/mpeg" => "mp3".to_string(),
        "audio/mp3" => "mp3".to_string(),
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav".to_string(),
        "audio/ogg" => "ogg".to_string(),
        "audio/webm" => "webm".to_string(),
        "audio/aac" => "aac".to_string(),
        "audio/mp4" | "audio/x-m4a" => "m4a".to_string(),
        "audio/flac" => "flac".to_string(),
        _ => "bin".to_string(),
    }
}

/// SECURITY: Best-effort path-containment check. Walks the candidate
/// path component-by-component and refuses any `..` segment that would
/// climb out of `base`, then confirms the lexical prefix matches.
/// We avoid `std::fs::canonicalize` here because it requires the file
/// to already exist (it doesn't yet — we're about to write it) and
/// because hitting the filesystem during request handling adds latency
/// for a check we can perform purely on the string form.
pub(crate) fn path_is_inside(candidate: &std::path::Path, base: &std::path::Path) -> bool {
    use std::path::Component;
    for component in candidate.components() {
        if matches!(component, Component::ParentDir) {
            return false;
        }
    }
    candidate.starts_with(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_canonical_audio_mimes() {
        for mime in [
            "audio/mpeg",
            "audio/mp3",
            "audio/ogg",
            "audio/wav",
            "audio/webm",
            "audio/aac",
        ] {
            assert!(is_allowed_audio_mime(mime), "{mime} should be allowed");
        }
    }

    #[test]
    fn allowlist_strips_charset_parameter() {
        assert!(is_allowed_audio_mime("audio/mpeg; charset=utf-8"));
    }

    #[test]
    fn allowlist_rejects_non_audio_types() {
        for mime in ["image/png", "application/octet-stream", "text/plain", ""] {
            assert!(!is_allowed_audio_mime(mime), "{mime} must be rejected");
        }
    }

    #[test]
    fn extension_falls_back_to_mime_when_filename_is_unsafe() {
        // `..` in the filename has no extension at all under Path's parser.
        assert_eq!(sanitize_extension("../../etc/passwd", "audio/mpeg"), "mp3");
        // Non-alphanumeric extension is rejected; mime takes over.
        assert_eq!(sanitize_extension("file.<script>", "audio/wav"), "wav");
    }

    #[test]
    fn extension_uses_mime_default_when_unknown_type() {
        assert_eq!(sanitize_extension("noext", "application/x-foo"), "bin");
    }

    #[test]
    fn file_path_to_url_keeps_streamer_subdir() {
        let url = file_path_to_url("/app/uploads/abc-123/def-456.mp3");
        assert_eq!(url, "/uploads/abc-123/def-456.mp3");
    }

    #[test]
    fn path_is_inside_accepts_normal_path() {
        let base = std::path::Path::new("/app/uploads");
        let candidate = std::path::Path::new("/app/uploads/user/file.mp3");
        assert!(path_is_inside(candidate, base));
    }

    #[test]
    fn path_is_inside_rejects_parent_dir_segment() {
        let base = std::path::Path::new("/app/uploads");
        let candidate = std::path::Path::new("/app/uploads/../etc/passwd");
        assert!(!path_is_inside(candidate, base));
    }

    #[test]
    fn path_is_inside_rejects_unrelated_prefix() {
        let base = std::path::Path::new("/app/uploads");
        let candidate = std::path::Path::new("/var/log/file");
        assert!(!path_is_inside(candidate, base));
    }
}
