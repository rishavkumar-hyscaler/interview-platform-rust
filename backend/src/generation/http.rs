//! Scheduling and status endpoints.
use super::provider::AiProvider;
use crate::{ApiError, Result, access::staff};
use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::sync::Arc;
use uuid::Uuid;

use super::PROMPT_VERSION;
#[derive(Clone)]
pub(super) struct ProviderOption(pub(super) Option<Arc<AiProvider>>);
pub fn routes(provider: Option<AiProvider>) -> Router<PgPool> {
    Router::new()
        .route("/api/v1/sessions/{session}/generation", get(status))
        .route(
            "/api/v1/jobs/{job}/question-sets",
            get(super::bank_http::status).post(super::bank_http::retry),
        )
        .layer(Extension(ProviderOption(provider.map(Arc::new))))
}
fn error(status: StatusCode, message: &'static str) -> ApiError {
    ApiError(status, message)
}
async fn status(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
    Extension(provider): Extension<ProviderOption>,
) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    let row=sqlx::query("SELECT coalesce(p.status,'not_started') AS status,CASE WHEN p.id IS NOT NULL THEN jsonb_build_object('id',p.id,'status',p.status,'attempts',p.attempts,'last_error',p.last_error) END AS job,j.blueprint FROM interview_sessions s JOIN jobs j ON j.id=s.job_id LEFT JOIN question_sets p ON p.id=s.question_set_id WHERE s.id=$1")
        .bind(session).fetch_optional(&mut *tx).await?.ok_or(error(StatusCode::NOT_FOUND,"Session not found"))?;
    let job: Option<Value> = row.get("job");
    let blueprint: Option<Value> = row.get("blueprint");
    let questions:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'position',position,'prompt',prompt,'kind',kind,'difficulty',difficulty,'competency_id',competency_id,'rubric',rubric,'answer',(SELECT jsonb_build_object('content',a.content,'revision',a.revision,'updated_at',a.updated_at) FROM interview_answers a WHERE a.question_id=session_question_details.id AND a.session_id=session_question_details.session_id)) FROM session_question_details() WHERE session_id=$1 ORDER BY position")
        .bind(session).fetch_all(&mut *tx).await?;
    let state: String = row.get("status");
    tx.commit().await?;
    Ok(Json(
        json!({"max_attempts":super::worker::MAX_ATTEMPTS,"status":state,"configured":provider.0.is_some(),"job":job,"blueprint":blueprint,"questions":questions,"prompt_version":PROMPT_VERSION}),
    ))
}
