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
    let row = sqlx::query("SELECT blueprint,preparation_duration_minutes FROM jobs WHERE id=$1")
        .bind(job)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(missing)?;
    let sets:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',p.id,'status',p.status,'duration_minutes',p.duration_minutes,'attempts',p.attempts,'last_error',p.last_error,'assigned',p.assigned_session_id IS NOT NULL,'last_failure',(SELECT jsonb_build_object('phase',a.metadata->'phase','summary',a.metadata->'summary','question_position',a.metadata->'question_position','findings',a.metadata->'findings') FROM audit_events a WHERE a.entity_id=p.id AND a.action='generation.rejected' ORDER BY a.created_at DESC,a.id DESC LIMIT 1),'question_count',(SELECT count(*) FROM interview_questions q WHERE q.set_id=p.id)) FROM question_sets p WHERE p.job_id=$1 ORDER BY p.created_at DESC,p.id DESC LIMIT 60")
        .bind(job).fetch_all(&mut *tx).await?;
    let questions:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',q.id,'set_id',q.set_id,'position',q.position,'kind',q.kind,'difficulty',q.difficulty,'prompt',q.prompt,'rubric',q.rubric) FROM interview_questions q JOIN question_sets p ON p.id=q.set_id WHERE p.job_id=$1 AND p.assigned_session_id IS NULL ORDER BY p.created_at,p.id,q.position LIMIT 120")
        .bind(job).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"configured":provider.0.is_some(),"target":bank::POOL_SIZE,"duration_minutes":row.get::<i32,_>("preparation_duration_minutes"),"blueprint":row.get::<Option<Value>,_>("blueprint"),"sets":sets,"questions":questions}),
    ))
}

pub(super) async fn retry(
    State(pool): State<PgPool>,
    Path(job): Path<Uuid>,
    headers: HeaderMap,
    Extension(provider): Extension<ProviderOption>,
) -> Result<(StatusCode, Json<Value>)> {
    let (owner, mut tx) = staff(&pool, &headers).await?;
    let duration: i32 =
        sqlx::query_scalar("SELECT preparation_duration_minutes FROM jobs WHERE id=$1 FOR UPDATE")
            .bind(job)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
    if provider.0.is_none() {
        return Err(crate::ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "AI generation is not configured",
        ));
    }
    // Retries are explicit after three bounded attempts; never an unbounded paid loop.
    sqlx::query("UPDATE question_sets SET status='pending',attempts=0,last_error=NULL,claim_token=NULL,lease_until=NULL,available_at=now() WHERE job_id=$1 AND status='failed' AND assigned_session_id IS NULL")
        .bind(job).execute(&mut *tx).await?;
    bank::replenish(&mut tx, owner, job, duration).await?;
    crate::audit::record(
        &mut tx,
        owner,
        Some(owner),
        job,
        "question_sets.retried",
        json!({"target":bank::POOL_SIZE}),
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"status":"pending"}))))
}
