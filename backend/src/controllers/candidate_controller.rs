use crate::{
    Result, access::candidate, dto::candidate::CandidateSessionResponse,
    models::candidate::SessionSnapshot, services::candidate_service,
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use sqlx::PgPool;
pub(crate) async fn candidate_session(
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> Result<Json<CandidateSessionResponse>> {
    let (owner, id, tx) = candidate(&pool, &headers).await?;
    Ok(Json(response(
        candidate_service::session(owner, id, tx).await?,
    )))
}
pub(crate) async fn candidate_transition(
    State(pool): State<PgPool>,
    Path(action): Path<String>,
    headers: HeaderMap,
) -> Result<Json<CandidateSessionResponse>> {
    let (owner, id, tx) = candidate(&pool, &headers).await?;
    Ok(Json(response(
        candidate_service::transition(owner, id, tx, action).await?,
    )))
}
pub(crate) fn response(session: SessionSnapshot) -> CandidateSessionResponse {
    CandidateSessionResponse {
        id: session.id,
        role: session.role,
        interviewee_id: session.interviewee_id,
        job_title: session.job_title,
        job_code: session.job_code,
        state: session.state,
        duration_minutes: session.duration_minutes,
        expires_at: session.expires_at,
        deadline_at: session.deadline_at,
        server_time: session.server_time,
        started_at: session.started_at,
        submitted_at: session.submitted_at,
        generation_status: session.generation_status,
        questions_ready: session.questions_ready,
        answer_count: session.answer_count,
        question_count: session.question_count,
    }
}
