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
    media::mime(headers, bytes)
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
    let hash = hex::encode(Sha256::digest(&bytes));
    let (owner, session, mut tx) = candidate(&pool, &headers).await?;
    let mut task = crate::observability::TaskLog::start("proctor.evidence.upload", session);
    sqlx::query("SELECT id FROM interview_sessions WHERE id=$1 FOR UPDATE")
        .bind(session)
        .fetch_one(&mut *tx)
        .await?;
    let old:Option<Value>=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='proctor.evidence'").bind(asset).bind(session).fetch_optional(&mut *tx).await?;
    if let Some(old) = old {
        if old["event_id"] != json!(event) || old["sha256"] != hash || old["mime"] != mime {
            return Err(ApiError(StatusCode::CONFLICT, "Evidence ID already used"));
        }
        tx.commit().await?;
        task.finish(true);
        return Ok(Json(old));
    }
    let observed:Option<Value>=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='proctor.observed' AND created_at>clock_timestamp()-interval '15 minutes'").bind(event).bind(session).fetch_optional(&mut *tx).await?;
    let observed = observed
        .filter(|m| proctoring::evidence_kind(m["kind"].as_str().unwrap_or("")))
        .ok_or(ApiError(
            StatusCode::NOT_FOUND,
            "Recent media observation not found",
        ))?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='proctor.evidence'",
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await?;
    let same_kind:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events WHERE entity_id=$1 AND action='proctor.evidence' AND metadata->>'event_id'=$2 AND (metadata->>'mime' LIKE 'image/%')=$3)").bind(session).bind(event.to_string()).bind(mime.starts_with("image/")).fetch_one(&mut *tx).await?;
    if count >= 40 || same_kind {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Evidence attachment limit reached",
        ));
    }
    let mut metadata = json!({"asset_id":asset,"event_id":event,"mime":mime,"bytes":bytes.len(),"sha256":hash,"parent_signature":observed["signature"],"source":"browser","verified":false,"capture":"post_signal","notice":"Browser-submitted media; server signature proves receipt integrity only"});
    metadata["signature"] = json!(proctoring::sign(owner, session, asset, &metadata).map_err(
        |_| ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Evidence signing unavailable"
        )
    )?);
    let file = media::path(owner, session, asset).with_extension("evidence");
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
        media::path(owner, session, asset).with_extension("evidence"),
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
}
