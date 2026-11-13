//! Signed, bounded evidence attachments with immutable storage and authorized reads.
use crate::{
    ApiError, Result, media, models::media::Asset, proctoring,
    repositories::evidence_repository as repo,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) const MAX_BYTES: usize = 2 * 1024 * 1024;
pub(crate) struct UploadEvidence {
    pub content_type: String,
    pub source: String,
    pub capture: String,
    pub surface: String,
    pub captured: Option<Option<i64>>,
}
fn format(content_type: &str, bytes: &[u8]) -> Result<&'static str> {
    if content_type == "image/jpeg" {
        if bytes.len() <= 512 * 1024
            && bytes.starts_with(&[0xff, 0xd8, 0xff])
            && bytes.ends_with(&[0xff, 0xd9])
        {
            return Ok("image/jpeg");
        }
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid or oversized JPEG evidence",
        ));
    }
    if content_type == "audio/wav" {
        return wave_format(bytes);
    }
    media::mime(content_type, bytes)
}
// Only the browser's bounded mono PCM16 evidence format is accepted here.
fn wave_format(bytes: &[u8]) -> Result<&'static str> {
    if bytes.len() >= 46 {
        let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
        if &bytes[..4] == b"RIFF"
            && &bytes[8..16] == b"WAVEfmt "
            && bytes[16..24] == [16, 0, 0, 0, 1, 0, 1, 0]
            && bytes[32..40] == [2, 0, 16, 0, b'd', b'a', b't', b'a']
            && (8000..=96000).contains(&rate)
            && size > 0
            && size.is_multiple_of(2)
            && size <= rate * 2 * 3
            && bytes.len() == 44 + size
            && u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize == bytes.len() - 8
            && u32::from_le_bytes(bytes[28..32].try_into().unwrap()) as usize == rate * 2
        {
            return Ok("audio/wav");
        }
    }
    Err(ApiError(
        StatusCode::BAD_REQUEST,
        "Invalid speech evidence WAV",
    ))
}
pub(crate) async fn upload(
    owner: Uuid,
    session: Uuid,
    mut tx: Transaction<'_, Postgres>,
    event: Uuid,
    asset: Uuid,
    input: UploadEvidence,
    bytes: &[u8],
) -> Result<Value> {
    if bytes.len() < 16 || bytes.len() > MAX_BYTES {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Evidence must be 16 bytes to 2 MB",
        ));
    }
    let mime = format(&input.content_type, bytes)?;
    let source = input.source.as_str();
    if !matches!(source, "screen" | "camera") || (source == "screen" && mime != "image/jpeg") {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid evidence source or screenshot format",
        ));
    }
    let capture = input.capture.as_str();
    if !matches!(capture, "post_signal" | "signal_window")
        || (capture == "signal_window" && mime != "audio/wav")
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid evidence capture mode",
        ));
    }
    let surface = input.surface.as_str();
    if !matches!(surface, "monitor" | "window" | "browser" | "unknown") {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid capture surface"));
    }
    let captured = input.captured;
    if captured.is_some_and(|value| value.is_none()) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid capture timestamp",
        ));
    }
    let hash = hex::encode(Sha256::digest(bytes));
    let mut task = crate::observability::TaskLog::start("proctor.evidence.upload", session);
    repo::lock_session(&mut tx, session).await?;
    let old = repo::existing(&mut tx, asset, session).await?;
    if let Some(old) = old {
        if old["event_id"] != json!(event)
            || old["sha256"] != hash
            || old["mime"] != mime
            || old["capture"]
                .as_str()
                .unwrap_or("post_signal")
                != capture
            || old["evidence_source"]
                .as_str()
                .unwrap_or("camera")
                != source
            || (source == "screen"
                && (old["capture_surface"] != surface
                    || old["captured_at_ms"] != json!(captured.flatten())))
        {
            return Err(ApiError(StatusCode::CONFLICT, "Evidence ID already used"));
        }
        tx.commit().await?;
        task.finish(true);
        return Ok(old);
    }
    let observed = repo::observed(&mut tx, event, session).await?;
    let observed = observed
        .filter(|m| {
            if source == "screen" {
                proctoring::screen_evidence_kind(m["kind"].as_str().unwrap_or(""))
                    && m["stage"] == "interview"
            } else {
                proctoring::evidence_kind(m["kind"].as_str().unwrap_or(""))
            }
        })
        .ok_or(ApiError(
            StatusCode::NOT_FOUND,
            "Recent matching observation not found",
        ))?;
    let count = repo::count(&mut tx, session, source).await?;
    let same_kind =
        repo::same_kind(&mut tx, session, event, mime.starts_with("image/"), source).await?;
    if count >= if source == "screen" { 120 } else { 40 } || same_kind {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Evidence attachment limit reached",
        ));
    }
    let mut metadata = json!({"asset_id":asset,"event_id":event,"mime":mime,"bytes":bytes.len(),"sha256":hash,"parent_signature":observed["signature"],"source":"browser","evidence_source":source,"capture_surface":surface,"captured_at_ms":captured.flatten(),"verified":false,"capture":capture,"notice":"Browser-submitted media; server signature proves receipt integrity only"});
    metadata["signature"] = json!(proctoring::sign(owner, session, asset, &metadata).map_err(
        |_| ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Evidence signing unavailable"
        )
    )?);
    let file = if source == "screen" {
        media::screen_snapshot_path(owner, session, asset)
    } else {
        media::path(owner, session, asset).with_extension("evidence")
    };
    tokio::fs::create_dir_all(file.parent().unwrap())
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Evidence storage unavailable",
            )
        })?;
    if file.exists() {
        // No committed audit reference under this locked session: recover an orphan.
        tokio::fs::remove_file(&file)
            .await
            .map_err(|_| {
                ApiError(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Evidence recovery unavailable",
                )
            })?;
    }
    if media::write_file(&file, bytes)
        .await
        .is_err()
    {
        let _ = tokio::fs::remove_file(&file).await;
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Evidence storage unavailable",
        ));
    }
    let inserted = repo::insert(&mut tx, asset, owner, session, &metadata).await;
    if let Err(error) = inserted {
        let _ = tokio::fs::remove_file(&file).await;
        return Err(error.into());
    }
    tx.commit().await?;
    task.finish(true);
    Ok(metadata)
}
pub(crate) async fn read(
    owner: Uuid,
    session: Uuid,
    mut tx: Transaction<'_, Postgres>,
    asset: Uuid,
) -> Result<Asset> {
    let metadata = repo::existing(&mut tx, asset, session)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "Evidence not found"))?;
    tx.commit().await?;
    let mut unsigned = metadata.clone();
    let signature = unsigned
        .as_object_mut()
        .and_then(|o| o.remove("signature"));
    if proctoring::sign(owner, session, asset, &unsigned)
        .ok()
        .as_deref()
        != signature
            .as_ref()
            .and_then(Value::as_str)
    {
        return Err(ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Evidence signature check failed",
        ));
    }
    media::read_asset(
        if metadata["evidence_source"] == "screen" {
            media::screen_snapshot_path(owner, session, asset)
        } else {
            media::path(owner, session, asset).with_extension("evidence")
        },
        metadata,
        asset,
    )
    .await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_mislabeled_images() {
        assert!(format("image/jpeg", b"arbitrary executable content").is_err());
        assert!(format("image/jpeg", &[0xff, 0xd8, 0xff, 0, 0xff, 0xd9]).is_ok());
    }
    #[test]
    fn accepts_only_bounded_pcm_speech_windows() {
        let mut bytes = vec![0u8; 44 + 16000 * 2 * 3];
        bytes[..4].copy_from_slice(b"RIFF");
        let size = bytes.len();
        bytes[4..8].copy_from_slice(&((size - 8) as u32).to_le_bytes());
        bytes[8..16].copy_from_slice(b"WAVEfmt ");
        bytes[16..24].copy_from_slice(&[16, 0, 0, 0, 1, 0, 1, 0]);
        bytes[24..28].copy_from_slice(&16000u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&32000u32.to_le_bytes());
        bytes[32..40].copy_from_slice(&[2, 0, 16, 0, b'd', b'a', b't', b'a']);
        bytes[40..44].copy_from_slice(&((size - 44) as u32).to_le_bytes());
        assert_eq!(wave_format(&bytes).unwrap(), "audio/wav");
        bytes.push(0);
        assert!(wave_format(&bytes).is_err());
        bytes.pop();
        bytes[24..28].copy_from_slice(&8000u32.to_le_bytes());
        assert!(wave_format(&bytes).is_err());
        assert!(wave_format(b"not audio").is_err());
    }
}
