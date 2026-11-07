//! Session HTTP extraction and response mapping; no SQL.
use crate::{
    Result, authenticate,
    dto::interview::*,
    models::interview::{Session, SessionListItem},
    pagination::Page,
    services::interview_service,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) async fn list_sessions(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<SessionListResponse>> {
    let limit = page.limit()?;
    let (owner, _) = authenticate(&pool, &headers).await?;
    let result = interview_service::list(&pool, owner, page.cursor, limit).await?;
    Ok(Json(SessionListResponse {
        items: result.items.into_iter().map(list_item).collect(),
        next_cursor: result.next_cursor,
    }))
}
pub(crate) async fn session_detail(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<SessionDetailResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    let result = interview_service::detail(&pool, owner, id).await?;
    Ok(Json(SessionDetailResponse {
        item: list_item(result.item),
        description: result.description,
        question_count: result.question_count,
        answer_count: result.answer_count,
        invitation_recoverable: result.invitation_recoverable,
        invitation_active: result.invitation_active,
    }))
}
pub(crate) async fn summary(
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> Result<Json<SummaryResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    let result = interview_service::summary(&pool, owner).await?;
    Ok(Json(SummaryResponse {
        jobs: result.jobs,
        sessions: result.sessions,
        active: result.active,
        reviewable: result.reviewable,
    }))
}
pub(crate) fn response(session: Session) -> SessionResponse {
    SessionResponse {
        id: session.id,
        interviewer_id: session.interviewer_id,
        job_id: session.job_id,
        question_set_id: session.question_set_id,
        candidate_name: session.candidate_name,
        candidate_email: session.candidate_email,
        state: session.state,
        duration_minutes: session.duration_minutes,
        expires_at: session.expires_at,
        started_at: session.started_at,
        deadline_at: session.deadline_at,
        submitted_at: session.submitted_at,
        created_at: session.created_at,
        interviewee_id: session.interviewee_id,
    }
}
fn list_item(item: SessionListItem) -> SessionListItemResponse {
    SessionListItemResponse {
        session: response(item.session),
        job_title: item.job_title,
        job_code: item.job_code,
    }
}
