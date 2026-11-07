//! Recording HTTP endpoints; persistence and storage are handled below this layer.
use crate::{
    ApiError, Result,
    access::{candidate, staff},
    dto::media::{RecordingResponse, RecordingsResponse},
    models::media::Asset,
    services::media_service,
};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use media_service::MAX_BYTES;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
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

async fn upload(
    State(pool): State<PgPool>,
    Path((question, recording)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Result<Json<RecordingResponse>> {
    let (owner, session, tx) = candidate(&pool, &headers).await?;
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let result = media_service::upload(
        owner,
        session,
        tx,
        question,
        recording,
        content_type,
        &bytes,
    )
    .await?;
    Ok(Json(recording_response(result)?))
}
async fn candidate_list(
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> Result<Json<RecordingsResponse>> {
    let (_, session, tx) = candidate(&pool, &headers).await?;
    list_response(media_service::list(session, tx, false).await?)
}
async fn staff_list(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<RecordingsResponse>> {
    let (_, tx) = staff(&pool, &headers).await?;
    list_response(media_service::list(session, tx, true).await?)
}
async fn candidate_read(
    State(pool): State<PgPool>,
    Path(recording): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response> {
    let (owner, session, tx) = candidate(&pool, &headers).await?;
    tx.commit().await?;
    Ok(asset_response(
        media_service::read(&pool, owner, session, recording).await?,
    ))
}
async fn staff_read(
    State(pool): State<PgPool>,
    Path((session, recording)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Response> {
    let (owner, tx) = staff(&pool, &headers).await?;
    tx.commit().await?;
    Ok(asset_response(
        media_service::read(&pool, owner, session, recording).await?,
    ))
}
fn recording_response(value: Value) -> Result<RecordingResponse> {
    serde_json::from_value(value).map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Recording metadata unavailable",
        )
    })
}
fn list_response(values: Vec<Value>) -> Result<Json<RecordingsResponse>> {
    Ok(Json(RecordingsResponse {
        recordings: values
            .into_iter()
            .map(recording_response)
            .collect::<Result<_>>()?,
    }))
}
pub(crate) fn asset_response(asset: Asset) -> Response {
    (
        [
            (header::CONTENT_TYPE, asset.mime),
            (header::CONTENT_DISPOSITION, "inline".into()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
            (header::CACHE_CONTROL, "private, no-store".into()),
            (header::ETAG, asset.etag),
        ],
        asset.bytes,
    )
        .into_response()
}
