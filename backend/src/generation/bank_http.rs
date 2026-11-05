//! Interviewer-only pool inspection and explicit retry of failed sets.
use super::{bank, http::ProviderOption};
use crate::{Result, access::staff, error::missing};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub(super) async fn status(
    State(pool): State<PgPool>,
    Path(job): Path<Uuid>,
    headers: HeaderMap,
    Extension(provider): Extension<ProviderOption>,
) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    let row = sqlx::query(
        "SELECT blueprint,preparation_duration_minutes,question_set_count FROM jobs WHERE id=$1",
    )
    .bind(job)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(missing)?;
    let sets:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',p.id,'status',p.status,'duration_minutes',p.duration_minutes,'attempts',p.attempts,'last_error',p.last_error,'assigned',false,'plan',(SELECT a.metadata->'plan' FROM audit_events a WHERE a.entity_id=p.id AND a.action='generation.planned' ORDER BY a.created_at,a.id LIMIT 1),'usage_count',(SELECT count(*) FROM interview_sessions s WHERE s.question_set_id=p.id),'last_failure',(SELECT jsonb_build_object('phase',a.metadata->'phase','summary',a.metadata->'summary','question_position',a.metadata->'question_position','findings',a.metadata->'findings') FROM audit_events a WHERE a.entity_id=p.id AND a.action='generation.rejected' ORDER BY a.created_at DESC,a.id DESC LIMIT 1),'question_count',(SELECT count(*) FROM interview_questions q WHERE q.set_id=p.id)) FROM question_sets p WHERE p.job_id=$1 AND p.id IN (SELECT id FROM question_sets WHERE job_id=$1 ORDER BY created_at,id LIMIT (SELECT question_set_count FROM jobs WHERE id=$1)) ORDER BY p.created_at DESC,p.id DESC LIMIT 60")
        .bind(job).fetch_all(&mut *tx).await?;
    let questions:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',q.id,'set_id',q.set_id,'position',q.position,'kind',q.kind,'difficulty',q.difficulty,'prompt',q.prompt,'rubric',q.rubric) FROM interview_questions q JOIN question_sets p ON p.id=q.set_id WHERE p.job_id=$1 AND p.id IN (SELECT id FROM question_sets WHERE job_id=$1 ORDER BY created_at,id LIMIT (SELECT question_set_count FROM jobs WHERE id=$1)) ORDER BY p.created_at,p.id,q.position LIMIT 120")
        .bind(job).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"max_attempts":super::worker::MAX_ATTEMPTS,"configured":provider.0.is_some(),"target":row.get::<i32,_>("question_set_count"),"duration_minutes":row.get::<i32,_>("preparation_duration_minutes"),"blueprint":row.get::<Option<Value>,_>("blueprint"),"sets":sets,"questions":questions}),
    ))
}

pub(super) async fn retry(
    State(pool): State<PgPool>,
    Path(job): Path<Uuid>,
    headers: HeaderMap,
    Extension(provider): Extension<ProviderOption>,
) -> Result<(StatusCode, Json<Value>)> {
    let (owner, mut tx) = staff(&pool, &headers).await?;
    let job_row = sqlx::query(
        "SELECT preparation_duration_minutes,question_set_count FROM jobs WHERE id=$1 FOR UPDATE",
    )
    .bind(job)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(missing)?;
    let duration: i32 = job_row.get("preparation_duration_minutes");
    if provider.0.is_none() {
        return Err(crate::ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "AI generation is not configured",
        ));
    }
    // Manual retry starts a fresh initial attempt plus three automatic retries.
    sqlx::query("UPDATE question_sets SET status='pending',attempts=0,last_error=NULL,claim_token=NULL,lease_until=NULL,available_at=now() WHERE job_id=$1 AND status='failed' AND id IN (SELECT id FROM question_sets WHERE job_id=$1 ORDER BY created_at,id LIMIT (SELECT question_set_count FROM jobs WHERE id=$1))")
        .bind(job).execute(&mut *tx).await?;
    bank::replenish(&mut tx, owner, job, duration).await?;
    crate::audit::record(
        &mut tx,
        owner,
        Some(owner),
        job,
        "question_sets.retried",
        json!({"target":job_row.get::<i32,_>("question_set_count")}),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"status":"pending"}))))
}
