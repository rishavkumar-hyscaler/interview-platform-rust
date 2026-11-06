//! Private, bounded event attachments. Separate from answer recordings and scoring.
use crate::{
    ApiError, Result,
    access::{candidate, staff},
    media, proctoring,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
const MAX_BYTES: usize = 2 * 1024 * 1024;
pub fn routes() -> Router<PgPool> {
    Router::new()
        .route(
            "/api/v1/candidate/events/{event}/evidence/{asset}",
            post(upload).layer(DefaultBodyLimit::max(MAX_BYTES)),
        )
        .route("/api/v1/sessions/{session}/evidence/{asset}", get(read))
}
fn format(headers: &HeaderMap, bytes: &[u8]) -> Result<&'static str> {
    if headers.get("content-type").and_then(|v| v.to_str().ok()) == Some("image/jpeg") {
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
    if headers.get("content-type").and_then(|v| v.to_str().ok()) == Some("audio/wav") {
        return wave_format(bytes);
    }
    media::mime(headers, bytes)
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
            && size % 2 == 0
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
async fn upload(
    State(pool): State<PgPool>,
    Path((event, asset)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<Value>> {
    if bytes.len() < 16 || bytes.len() > MAX_BYTES {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Evidence must be 16 bytes to 2 MB",
        ));
    }
    let mime = format(&headers, &bytes)?;
    let source = headers
        .get("x-evidence-source")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("camera");
    if !matches!(source, "screen" | "camera") || (source == "screen" && mime != "image/jpeg") {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid evidence source or screenshot format",
        ));
    }
    let capture = headers
        .get("x-evidence-capture")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("post_signal");
    if !matches!(capture, "post_signal" | "signal_window")
        || (capture == "signal_window" && mime != "audio/wav")
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid evidence capture mode",
        ));
    }
    let surface = headers
        .get("x-capture-surface")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    if !matches!(surface, "monitor" | "window" | "browser" | "unknown") {
        return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid capture surface"));
    }
    let captured = headers.get("x-evidence-captured-at").map(|v| {
        v.to_str()
            .ok()
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|n| (0..=4_102_444_800_000).contains(n))
    });
    if captured.is_some_and(|value| value.is_none()) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid capture timestamp",
        ));
    }
    let hash = hex::encode(Sha256::digest(&bytes));
    let (owner, session, mut tx) = candidate(&pool, &headers).await?;
    let mut task = crate::observability::TaskLog::start("proctor.evidence.upload", session);
    sqlx::query("SELECT id FROM interview_sessions WHERE id=$1 FOR UPDATE")
        .bind(session)
        .fetch_one(&mut *tx)
        .await?;
    let old:Option<Value>=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='proctor.evidence'").bind(asset).bind(session).fetch_optional(&mut *tx).await?;
    if let Some(old) = old {
        if old["event_id"] != json!(event)
            || old["sha256"] != hash
            || old["mime"] != mime
            || old["capture"].as_str().unwrap_or("post_signal") != capture
            || old["evidence_source"].as_str().unwrap_or("camera") != source
            || (source == "screen"
                && (old["capture_surface"] != surface
                    || old["captured_at_ms"] != json!(captured.flatten())))
        {
            return Err(ApiError(StatusCode::CONFLICT, "Evidence ID already used"));
        }
        tx.commit().await?;
        task.finish(true);
        return Ok(Json(old));
    }
    let observed:Option<Value>=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='proctor.observed' AND created_at>clock_timestamp()-interval '15 minutes'").bind(event).bind(session).fetch_optional(&mut *tx).await?;
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
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='proctor.evidence' AND coalesce(metadata->>'evidence_source','camera')=$2",
    )
    .bind(session)
    .bind(source)
    .fetch_one(&mut *tx)
    .await?;
    let same_kind:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events WHERE entity_id=$1 AND action='proctor.evidence' AND metadata->>'event_id'=$2 AND (metadata->>'mime' LIKE 'image/%')=$3 AND coalesce(metadata->>'evidence_source','camera')=$4)").bind(session).bind(event.to_string()).bind(mime.starts_with("image/")).bind(source).fetch_one(&mut *tx).await?;
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
        tokio::fs::remove_file(&file).await.map_err(|_| {
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Evidence recovery unavailable",
            )
        })?;
    }
    if media::write_file(&file, &bytes).await.is_err() {
        let _ = tokio::fs::remove_file(&file).await;
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Evidence storage unavailable",
        ));
    }
    let inserted=sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'proctor.evidence',$4)").bind(asset).bind(owner).bind(session).bind(&metadata).execute(&mut *tx).await;
    if let Err(error) = inserted {
        let _ = tokio::fs::remove_file(&file).await;
        return Err(error.into());
    }
    tx.commit().await?;
    task.finish(true);
    Ok(Json(metadata))
}
async fn read(
    State(pool): State<PgPool>,
    Path((session, asset)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    let (owner, mut tx) = staff(&pool, &headers).await?;
    let metadata:Value=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='proctor.evidence'").bind(asset).bind(session).fetch_optional(&mut *tx).await?.ok_or(ApiError(StatusCode::NOT_FOUND,"Evidence not found"))?;
    tx.commit().await?;
    let mut unsigned = metadata.clone();
    let signature = unsigned.as_object_mut().and_then(|o| o.remove("signature"));
    if proctoring::sign(owner, session, asset, &unsigned)
        .ok()
        .as_deref()
        != signature.as_ref().and_then(Value::as_str)
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
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "image/jpeg".parse().unwrap());
        assert!(format(&headers, b"arbitrary executable content").is_err());
        assert!(format(&headers, &[0xff, 0xd8, 0xff, 0, 0xff, 0xd9]).is_ok());
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
