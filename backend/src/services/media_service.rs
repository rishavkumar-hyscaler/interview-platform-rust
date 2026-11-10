//! Bounded immutable recording storage and upload lifecycle.
use crate::{
    ApiError, Result, digest,
    infrastructure::{finalize_due_session, interviewer_transaction},
    models::media::Asset,
    repositories::media_repository as repo,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
pub(crate) const MAX_BYTES: usize = 15 * 1024 * 1024;
fn root() -> PathBuf {
    std::env::var_os("MEDIA_STORAGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("storage/media"))
}
pub(crate) fn path(owner: Uuid, session: Uuid, recording: Uuid) -> PathBuf {
    root()
        .join(owner.to_string())
        .join(session.to_string())
        .join(recording.to_string())
}
pub(crate) fn screen_snapshot_path(owner: Uuid, session: Uuid, asset: Uuid) -> PathBuf {
    root()
        .join(owner.to_string())
        .join(session.to_string())
        .join("screenshots")
        .join(format!("{asset}.jpg"))
}
pub(crate) fn mime(content_type: &str, bytes: &[u8]) -> Result<&'static str> {
    let input = content_type
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
pub(crate) async fn upload(
    owner: Uuid,
    session: Uuid,
    mut tx: Transaction<'_, Postgres>,
    question: Uuid,
    recording: Uuid,
    content_type: &str,
    bytes: &[u8],
) -> Result<Value> {
    if bytes.len() < 16 || bytes.len() > MAX_BYTES {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Recording must be between 16 bytes and 15 MB",
        ));
    }
    let mime = mime(content_type, bytes)?;
    let hash = hex::encode(Sha256::digest(&bytes));
    let mut task = crate::observability::TaskLog::start("recording.upload", session);
    finalize_due_session(&mut tx, owner, session).await?;
    let old = repo::recording(&mut tx, recording, session).await?;
    if let Some(old) = old {
        if old["sha256"] != hash || old["question_id"] != json!(question) || old["mime"] != mime {
            return Err(ApiError(StatusCode::CONFLICT, "Recording ID already used"));
        }
        tx.commit().await?;
        task.finish(true);
        return Ok(old);
    }
    let state = repo::state(&mut tx, session).await?;
    if state != "in_progress" {
        tx.commit().await?;
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Interview ended; recording remains on this device and was not submitted",
        ));
    }
    let exists = repo::question_exists(&mut tx, question, session).await?;
    if !exists {
        return Err(ApiError(StatusCode::NOT_FOUND, "Question not found"));
    }
    let used = repo::used(&mut tx, session).await?;
    if used >= 12 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Maximum 12 recordings per interview",
        ));
    }
    let file = path(owner, session, recording);
    let parent = file.parent().unwrap();
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|_| {
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
                tokio::fs::remove_file(&file)
                    .await
                    .map_err(|_| {
                        ApiError(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "Unable to recover interrupted upload",
                        )
                    })?;
                write_file(&file, &bytes)
                    .await
                    .map_err(|_| {
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
    let active = repo::active(&mut tx, session).await?;
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
    let inserted = repo::insert(&mut tx, recording, owner, session, &metadata).await;
    if let Err(error) = inserted {
        if created {
            let _ = tokio::fs::remove_file(&file).await;
        }
        return Err(error.into());
    }
    // A failed commit is ambiguous: retain the immutable file for reconciliation, never delete a potentially committed attachment.
    tx.commit().await?;
    task.finish(true);
    Ok(metadata)
}
pub(crate) async fn write_file(file: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut handle = options.open(file).await?;
    handle.write_all(bytes).await?;
    handle.sync_all().await
}
pub(crate) async fn list(
    session: Uuid,
    mut tx: Transaction<'_, Postgres>,
    require_session: bool,
) -> Result<Vec<Value>> {
    if require_session && !repo::session_exists(&mut tx, session).await? {
        return Err(ApiError(StatusCode::NOT_FOUND, "Session not found"));
    }
    let recordings = repo::list(&mut tx, session).await?;
    tx.commit().await?;
    Ok(recordings)
}
pub(crate) async fn read(
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
    recording: Uuid,
) -> Result<Asset> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let metadata = repo::recording(&mut tx, recording, session)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "Recording not found"))?;
    tx.commit().await?;
    read_asset(path(owner, session, recording), metadata, recording).await
}
pub(crate) async fn read_asset(file: PathBuf, metadata: Value, recording: Uuid) -> Result<Asset> {
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
    if hex::encode(Sha256::digest(&bytes))
        != metadata["sha256"]
            .as_str()
            .unwrap_or("")
    {
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
    Ok(Asset {
        bytes,
        mime,
        etag,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_spoofed_recording_types() {
        assert!(mime("video/webm", b"this is executable text").is_err());
        assert_eq!(
            mime("video/webm", &[0x1a, 0x45, 0xdf, 0xa3, 0]).unwrap(),
            "video/webm"
        );
    }
}

pub(crate) async fn remove_session_media(owner: Uuid, session: Uuid) {
    let directory = root()
        .join(owner.to_string())
        .join(session.to_string());
    match tokio::fs::remove_dir_all(directory).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::error!(%session, %error, "Job deletion could not remove session media")
        }
    }
}
