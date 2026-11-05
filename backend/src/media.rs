//! Private bounded answer recordings, with lightweight immutable audit references.
use crate::{
    ApiError, Result, digest,
    infrastructure::{finalize_due_session, interviewer_transaction},
    interviews::{candidate, staff},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
const MAX_BYTES: usize = 15 * 1024 * 1024;
pub fn routes() -> Router<PgPool> {
    Router::new()
        .route(
            "/api/v1/candidate/questions/{question}/recordings/{recording}",
            post(upload).layer(DefaultBodyLimit::max(MAX_BYTES)),
        )
        .route("/api/v1/candidate/recordings", get(candidate_list))
        .route(
            "/api/v1/candidate/recordings/{recording}",
            get(candidate_read),
        )
        .route("/api/v1/sessions/{session}/recordings", get(staff_list))
        .route(
            "/api/v1/sessions/{session}/recordings/{recording}",
            get(staff_read),
        )
}
fn root() -> PathBuf {
    std::env::var_os("MEDIA_STORAGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("storage/media"))
}
fn path(owner: Uuid, session: Uuid, recording: Uuid) -> PathBuf {
    root()
        .join(owner.to_string())
        .join(session.to_string())
        .join(recording.to_string())
}
fn mime(headers: &HeaderMap, bytes: &[u8]) -> Result<&'static str> {
    let input = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    let ebml = bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]);
    let ogg = bytes.starts_with(b"OggS");
    let mp4 = bytes.len() > 12 && &bytes[4..8] == b"ftyp";
    match input {
        "audio/webm" if ebml => Ok("audio/webm"),
        "video/webm" if ebml => Ok("video/webm"),
        "audio/ogg" if ogg => Ok("audio/ogg"),
        "audio/mp4" if mp4 => Ok("audio/mp4"),
        "video/mp4" if mp4 => Ok("video/mp4"),
        _ => Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Unsupported recording format",
        )),
    }
}
async fn upload(
    State(pool): State<PgPool>,
    Path((question, recording)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<Value>> {
    if bytes.len() < 16 || bytes.len() > MAX_BYTES {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Recording must be between 16 bytes and 15 MB",
        ));
    }
    let mime = mime(&headers, &bytes)?;
    let hash = hex::encode(Sha256::digest(&bytes));
    let (owner, session, mut tx) = candidate(&pool, &headers).await?;
    finalize_due_session(&mut tx, owner, session).await?;
    let old:Option<Value>=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='answer.recorded'").bind(recording).bind(session).fetch_optional(&mut *tx).await?;
    if let Some(old) = old {
        if old["sha256"] != hash || old["question_id"] != json!(question) || old["mime"] != mime {
            return Err(ApiError(StatusCode::CONFLICT, "Recording ID already used"));
        }
        tx.commit().await?;
        return Ok(Json(old));
    }
    let state: String = sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1")
        .bind(session)
        .fetch_one(&mut *tx)
        .await?;
    if state != "in_progress" {
        tx.commit().await?;
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Interview ended; recording remains on this device and was not submitted",
        ));
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM interview_questions WHERE id=$1 AND session_id=$2)",
    )
    .bind(question)
    .bind(session)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(ApiError(StatusCode::NOT_FOUND, "Question not found"));
    }
    let used: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='answer.recorded'",
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await?;
    if used >= 12 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Maximum 12 recordings per interview",
        ));
    }
    let file = path(owner, session, recording);
    let parent = file.parent().unwrap();
    tokio::fs::create_dir_all(parent).await.map_err(|_| {
        ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Recording storage is unavailable",
        )
    })?;
    let created = match write_file(&file, &bytes).await {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let size = tokio::fs::metadata(&file)
                .await
                .map_err(|_| {
                    ApiError(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Recording storage unavailable",
                    )
                })?
                .len();
            let matches = if size <= MAX_BYTES as u64 {
                tokio::fs::read(&file)
                    .await
                    .is_ok_and(|existing| hex::encode(Sha256::digest(&existing)) == hash)
            } else {
                false
            };
            if matches {
                false
            } else {
                // No audit reference exists under this locked session. Recover a partial
                // orphan left by a cancelled upload, never overwrite a committed clip.
                tokio::fs::remove_file(&file).await.map_err(|_| {
                    ApiError(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Unable to recover interrupted upload",
                    )
                })?;
                write_file(&file, &bytes).await.map_err(|_| {
                    ApiError(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Recording could not be saved; retry",
                    )
                })?;
                true
            }
        }
        Err(_) => {
            let _ = tokio::fs::remove_file(&file).await;
            return Err(ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Recording storage unavailable; retry",
            ));
        }
    };
    finalize_due_session(&mut tx, owner, session).await?;
    let active: bool =
        sqlx::query_scalar("SELECT state='in_progress' FROM interview_sessions WHERE id=$1")
            .bind(session)
            .fetch_one(&mut *tx)
            .await?;
    if !active {
        if created {
            let _ = tokio::fs::remove_file(&file).await;
        }
        tx.commit().await?;
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Interview ended during upload; the recording was not submitted",
        ));
    }
    let metadata = json!({"recording_id":recording,"question_id":question,"mime":mime,"bytes":bytes.len(),"sha256":hash,"actor":"interviewee","assessment":"text transcript required for AI scoring; otherwise human review"});
    let inserted=sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'answer.recorded',$4)").bind(recording).bind(owner).bind(session).bind(&metadata).execute(&mut *tx).await;
    if let Err(error) = inserted {
        if created {
            let _ = tokio::fs::remove_file(&file).await;
        }
        return Err(error.into());
    }
    // A failed commit is ambiguous: retain the immutable file for reconciliation, never delete a potentially committed attachment.
    tx.commit().await?;
    Ok(Json(metadata))
}
async fn write_file(file: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut handle = options.open(file).await?;
    handle.write_all(bytes).await?;
    handle.sync_all().await
}
async fn list(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, session: Uuid) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar("SELECT metadata||jsonb_build_object('saved_at',created_at) FROM audit_events WHERE entity_id=$1 AND action='answer.recorded' ORDER BY created_at,id").bind(session).fetch_all(&mut **tx).await?)
}
async fn candidate_list(State(pool): State<PgPool>, headers: HeaderMap) -> Result<Json<Value>> {
    let (_, session, mut tx) = candidate(&pool, &headers).await?;
    let recordings = list(&mut tx, session).await?;
    tx.commit().await?;
    Ok(Json(json!({"recordings":recordings})))
}
async fn staff_list(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    let found: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM interview_sessions WHERE id=$1)")
            .bind(session)
            .fetch_one(&mut *tx)
            .await?;
    if !found {
        return Err(ApiError(StatusCode::NOT_FOUND, "Session not found"));
    }
    let recordings = list(&mut tx, session).await?;
    tx.commit().await?;
    Ok(Json(json!({"recordings":recordings})))
}
async fn candidate_read(
    State(pool): State<PgPool>,
    Path(recording): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    let (owner, session, tx) = candidate(&pool, &headers).await?;
    tx.commit().await?;
    read(&pool, owner, session, recording).await
}
async fn staff_read(
    State(pool): State<PgPool>,
    Path((session, recording)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    let (owner, tx) = staff(&pool, &headers).await?;
    tx.commit().await?;
    read(&pool, owner, session, recording).await
}
async fn read(
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
    recording: Uuid,
) -> Result<impl IntoResponse + use<>> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let row=sqlx::query("SELECT metadata FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='answer.recorded'").bind(recording).bind(session).fetch_optional(&mut *tx).await?.ok_or(ApiError(StatusCode::NOT_FOUND,"Recording not found"))?;
    let metadata: Value = row.get("metadata");
    tx.commit().await?;
    let file = path(owner, session, recording);
    let size = tokio::fs::metadata(&file)
        .await
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "Recording file unavailable"))?
        .len();
    if size > MAX_BYTES as u64 {
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Recording integrity check failed",
        ));
    }
    let bytes = tokio::fs::read(file)
        .await
        .map_err(|_| ApiError(StatusCode::NOT_FOUND, "Recording file unavailable"))?;
    if hex::encode(Sha256::digest(&bytes)) != metadata["sha256"].as_str().unwrap_or("") {
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Recording integrity check failed",
        ));
    }
    let mime = metadata["mime"]
        .as_str()
        .unwrap_or("application/octet-stream")
        .to_string();
    let etag = format!("\"{}\"", digest(&recording.to_string()));
    Ok((
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_DISPOSITION, "inline".into()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
            (header::ETAG, etag),
        ],
        bytes,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_spoofed_recording_types() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, "video/webm".parse().unwrap());
        assert!(mime(&headers, b"this is executable text").is_err());
        assert_eq!(
            mime(&headers, &[0x1a, 0x45, 0xdf, 0xa3, 0]).unwrap(),
            "video/webm"
        );
    }
}
