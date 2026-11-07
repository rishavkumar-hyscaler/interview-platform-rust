//! Generation HTTP entry points; SQL and retry decisions live below this layer.
use crate::{
    Result, authenticate,
    dto::generation::{PendingResponse, QuestionSetsResponse, SessionGenerationResponse},
    generation::{PROMPT_VERSION, provider::AiProvider, worker::MAX_ATTEMPTS},
    services::generation_service,
};
use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;
#[derive(Clone)]
struct ProviderOption(Option<Arc<AiProvider>>);
pub fn routes(provider: Option<AiProvider>) -> Router<PgPool> {
    Router::new()
        .route("/api/v1/sessions/{session}/generation", get(status))
        .route("/api/v1/jobs/{job}/question-sets", get(sets).post(retry))
        .layer(Extension(ProviderOption(provider.map(Arc::new))))
}
async fn status(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
    Extension(provider): Extension<ProviderOption>,
) -> Result<Json<SessionGenerationResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    let result = generation_service::session_status(&pool, owner, session).await?;
    Ok(Json(SessionGenerationResponse {
        max_attempts: MAX_ATTEMPTS,
        status: result.row.status,
        configured: provider.0.is_some(),
        job: result.row.job,
        blueprint: result.row.blueprint,
        questions: result.questions,
        prompt_version: PROMPT_VERSION,
    }))
}
async fn sets(
    State(pool): State<PgPool>,
    Path(job): Path<Uuid>,
    headers: HeaderMap,
    Extension(provider): Extension<ProviderOption>,
) -> Result<Json<QuestionSetsResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    let result = generation_service::set_status(&pool, owner, job).await?;
    Ok(Json(QuestionSetsResponse {
        max_attempts: MAX_ATTEMPTS,
        configured: provider.0.is_some(),
        target: result.row.question_set_count,
        duration_minutes: result.row.preparation_duration_minutes,
        blueprint: result.row.blueprint,
        sets: result.sets,
        questions: result.questions,
    }))
}
async fn retry(
    State(pool): State<PgPool>,
    Path(job): Path<Uuid>,
    headers: HeaderMap,
    Extension(provider): Extension<ProviderOption>,
) -> Result<(StatusCode, Json<PendingResponse>)> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    generation_service::retry(&pool, owner, job, provider.0.is_some()).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(PendingResponse { status: "pending" }),
    ))
}
