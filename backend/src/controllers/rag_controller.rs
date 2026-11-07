use crate::{
    Result, authenticate,
    dto::rag::{IndexJobDetails, IndexStatusResponse, RetryResponse},
    generation::provider::AiProvider,
    services::rag_service,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::get,
};
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) fn routes() -> Router<PgPool> {
    Router::new().route("/api/v1/sessions/{session}/rag", get(status).post(retry))
}
async fn status(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<IndexStatusResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    let result = rag_service::status(&pool, owner, session).await?;
    let mut response = IndexStatusResponse {
        status: if result.scored {
            "pending"
        } else {
            "awaiting_scoring"
        }
        .into(),
        attempts: 0,
        max_attempts: crate::rag::worker::MAX_ATTEMPTS,
        configured: AiProvider::from_environment().is_ok(),
        revision: None,
        indexed_revision: None,
        details: None,
    };
    if let Some(job) = result.job {
        response.status = job.status;
        response.attempts = job.attempts;
        response.revision = Some(job.revision);
        response.indexed_revision = Some(job.indexed_revision);
        response.details = Some(IndexJobDetails {
            last_error: job.last_error,
            model: job.model,
            dimensions: job.dimensions,
        });
    }
    Ok(Json(response))
}
async fn retry(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<RetryResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    rag_service::retry(&pool, owner, session).await?;
    Ok(Json(RetryResponse {
        status: "pending",
    }))
}
