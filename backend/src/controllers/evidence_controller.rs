//! Evidence HTTP metadata parsing and response mapping.
use crate::{
    ApiError, Result,
    access::{candidate, staff},
    dto::evidence::EvidenceResponse,
    services::evidence_service::{self, UploadEvidence},
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
};
use evidence_service::MAX_BYTES;
use sqlx::PgPool;
use uuid::Uuid;
pub fn routes() -> Router<PgPool> {
    Router::new()
        .route(
            "/api/v1/candidate/events/{event}/evidence/{asset}",
            post(upload).layer(DefaultBodyLimit::max(MAX_BYTES)),
        )
        .route("/api/v1/sessions/{session}/evidence/{asset}", get(read))
}

async fn upload(
    State(pool): State<PgPool>,
    Path((event, asset)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<EvidenceResponse>> {
    let (owner, session, tx) = candidate(&pool, &headers).await?;
    let input = UploadEvidence {
        content_type: text(&headers, "content-type", ""),
        source: text(&headers, "x-evidence-source", "camera"),
        capture: text(&headers, "x-evidence-capture", "post_signal"),
        surface: text(&headers, "x-capture-surface", "unknown"),
        captured: headers
            .get("x-evidence-captured-at")
            .map(|v| {
                v.to_str()
                    .ok()
                    .and_then(|v| v.parse::<i64>().ok())
                    .filter(|n| (0..=4_102_444_800_000).contains(n))
            }),
    };
    let value = evidence_service::upload(owner, session, tx, event, asset, input, &bytes).await?;
    let response = serde_json::from_value(value).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Evidence metadata unavailable",
        )
    })?;
    Ok(Json(response))
}
async fn read(
    State(pool): State<PgPool>,
    Path((session, asset)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response> {
    let (owner, tx) = staff(&pool, &headers).await?;
    Ok(super::media_controller::asset_response(
        evidence_service::read(owner, session, tx, asset).await?,
    ))
}
fn text(headers: &HeaderMap, key: &str, default: &str) -> String {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .unwrap_or(default)
        .to_owned()
}
