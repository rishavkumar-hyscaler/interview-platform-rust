//! Interviewer session lists, details and dashboard summary.
use crate::{
    Result,
    access::staff,
    error::missing,
    pagination::{Page, paginated},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;
pub(super) async fn list_sessions(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    let limit = page.limit()?;
    let (recruiter, mut tx) = staff(&pool, &headers).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT (to_jsonb(s)-'request_key'-'created_by') || jsonb_build_object('job_title',j.title,'job_code',j.job_code) FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.interviewer_id=$1 AND ($2::uuid IS NULL OR (s.created_at,s.id)<(SELECT created_at,id FROM interview_sessions WHERE id=$2)) ORDER BY s.created_at DESC,s.id DESC LIMIT $3")
        .bind(recruiter).bind(page.cursor).bind(limit+1).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(paginated(items, limit)))
}
pub(super) async fn session_detail(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (recruiter, mut tx) = staff(&pool, &headers).await?;
    let result: Value=sqlx::query_scalar("SELECT (to_jsonb(s)-'request_key'-'created_by') || jsonb_build_object('job_title',j.title,'job_code',j.job_code,'description',j.description,'question_count',(SELECT count(*) FROM interview_questions q WHERE q.session_id=s.id),'answer_count',(SELECT count(*) FROM interview_questions q WHERE q.session_id=s.id AND (EXISTS(SELECT 1 FROM interview_answers a WHERE a.question_id=q.id AND length(btrim(a.content))>0) OR EXISTS(SELECT 1 FROM audit_events m WHERE m.entity_id=q.session_id AND m.action='answer.recorded' AND m.metadata->>'question_id'=q.id::text))),'invitation_recoverable',coalesce((SELECT sealed_token IS NOT NULL FROM auth_sessions c WHERE c.interview_session_id=s.id),false),'invitation_active',s.state IN ('invited','ready','in_progress') AND least(s.expires_at,coalesce(s.deadline_at,s.expires_at))>now()) FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.id=$1 AND s.interviewer_id=$2")
        .bind(id).bind(recruiter).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    tx.commit().await?;
    Ok(Json(result))
}
pub(super) async fn summary(State(pool): State<PgPool>, headers: HeaderMap) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    let result: Value=sqlx::query_scalar("SELECT jsonb_build_object('jobs',(SELECT count(*) FROM jobs),'sessions',count(*),'active',count(*) FILTER(WHERE state IN ('ready','in_progress') AND least(expires_at,coalesce(deadline_at,expires_at))>now()),'reviewable',count(*) FILTER(WHERE state IN ('submitted','scoring','completed'))) FROM interview_sessions")
        .fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(result))
}
