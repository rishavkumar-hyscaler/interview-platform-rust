//! Scheduling and status endpoints.
use super::provider::AiProvider;
use crate::{ApiError, Result, access::staff, audit::record as audit};
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
        .route(
            "/api/v1/sessions/{session}/generation",
            get(status).post(enqueue),
        )
        .route(
            "/api/v1/jobs/{job}/question-sets",
            get(super::bank_http::status).post(super::bank_http::retry),
        )
        .layer(Extension(ProviderOption(provider.map(Arc::new))))
}
fn error(status: StatusCode, message: &'static str) -> ApiError {
    ApiError(status, message)
}
async fn enqueue(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
    Extension(provider): Extension<ProviderOption>,
) -> Result<(StatusCode, Json<Value>)> {
    let (user, mut tx) = staff(&pool, &headers).await?;
    let row=sqlx::query("SELECT state,expires_at>now() AS active,generation_status,generation_id,(SELECT count(*) FROM interview_questions q WHERE q.session_id=s.id) AS question_count FROM interview_sessions s WHERE id=$1 FOR UPDATE")
        .bind(session).fetch_optional(&mut *tx).await?.ok_or(error(StatusCode::NOT_FOUND,"Session not found"))?;
    let completed = row.get::<String, _>("generation_status") == "completed"
        || (row.get::<Option<Uuid>, _>("generation_id").is_none()
            && row.get::<i64, _>("question_count") > 0);
    if completed {
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(json!({"status":"completed"}))));
    }
    if !row.get::<bool, _>("active")
        || !matches!(row.get::<String, _>("state").as_str(), "invited" | "ready")
    {
        return Err(error(
            StatusCode::CONFLICT,
            "Questions can only be prepared before an active interview starts",
        ));
    }
    if super::bank::assign(&mut tx, user, session).await? {
        tx.commit().await?;
        return Ok((StatusCode::OK, Json(json!({"status":"completed"}))));
    }
    if provider.0.is_none() {
        return Err(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "AI generation is not configured",
        ));
    }
    let state: String = row.get("generation_status");
    if matches!(state.as_str(), "pending" | "running") {
        tx.commit().await?;
        return Ok((StatusCode::ACCEPTED, Json(json!({"status":state}))));
    }
    sqlx::query("UPDATE interview_sessions SET generation_id=coalesce(generation_id,$2),generation_seed=coalesce(generation_seed,$3),generation_status='pending',generation_attempts=0,generation_error=NULL,generation_available_at=now(),generation_claim_token=NULL,generation_lease_until=NULL WHERE id=$1")
        .bind(session).bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(&mut *tx).await?;
    audit(
        &mut tx,
        user,
        Some(user),
        session,
        "questions.queued",
        json!({"prompt_version":PROMPT_VERSION}),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"status":"pending"}))))
}
async fn status(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
    Extension(provider): Extension<ProviderOption>,
) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    let row=sqlx::query("SELECT s.job_id,s.generation_id,s.generation_status,CASE WHEN s.generation_id IS NOT NULL THEN jsonb_build_object('id',s.generation_id,'status',s.generation_status,'attempts',s.generation_attempts,'last_error',s.generation_error,'seed',s.generation_seed) END AS job,j.blueprint FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.id=$1")
        .bind(session).fetch_optional(&mut *tx).await?.ok_or(error(StatusCode::NOT_FOUND,"Session not found"))?;
    let job: Option<Value> = row.get("job");
    let blueprint: Option<Value> = row.get("blueprint");
    let questions:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'position',position,'prompt',prompt,'kind',kind,'difficulty',difficulty,'competency_id',competency_id,'rubric',rubric,'answer',(SELECT jsonb_build_object('content',a.content,'revision',a.revision,'updated_at',a.updated_at) FROM interview_answers a WHERE a.question_id=interview_questions.id)) FROM interview_questions WHERE session_id=$1 ORDER BY position")
        .bind(session).fetch_all(&mut *tx).await?;
    let state = if !questions.is_empty() && row.get::<Option<Uuid>, _>("generation_id").is_none() {
        "completed".to_string()
    } else {
        row.get::<String, _>("generation_status")
    };
    tx.commit().await?;
    Ok(Json(
        json!({"status":state,"configured":provider.0.is_some(),"job":job,"blueprint":blueprint,"questions":questions,"prompt_version":PROMPT_VERSION}),
    ))
}
