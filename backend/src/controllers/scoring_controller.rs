use crate::{
    Result, authenticate,
    dto::scoring::{AssessmentResponse, RetryResponse},
    generation::provider::AiProvider,
    services::scoring_service,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    routing::{get, post},
};
use sqlx::PgPool;
use uuid::Uuid;
pub fn routes() -> Router<PgPool> {
    Router::new()
        .route("/api/v1/sessions/{session}/assessment", get(report))
        .route("/api/v1/sessions/{session}/assessment/retry", post(retry))
}
async fn report(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<AssessmentResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    let r = scoring_service::report(&pool, owner, session).await?;
    let failed = r.assessment.is_none() && r.attempts >= 4 && r.state == "submitted";
    Ok(Json(AssessmentResponse {
        configured: AiProvider::from_environment().is_ok(),
        state: r.state,
        assessment: r.assessment,
        attempts: r.attempts,
        max_attempts: 4,
        last_failure: r.last_failure,
        failed,
        observations: r.observations,
        integrity_notice: "Browser observations are unverified signals, not proof of cheating. Human review required.",
    }))
}
async fn retry(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<RetryResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    scoring_service::retry(&pool, owner, session).await?;
    Ok(Json(RetryResponse { queued: true }))
}
