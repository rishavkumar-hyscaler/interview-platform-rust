//! Report exports and aggregate analytics.
use crate::{ApiError, Result, access::staff, audit::record as audit};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use super::exists;
use super::integrity::{integrity, weights};
pub(super) async fn export_report(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let integrity_report = integrity(State(pool.clone()), Path(session), headers.clone())
        .await?
        .0;
    let (owner, mut tx) = staff(&pool, &headers).await?;
    exists(&mut tx, session).await?;
    let interview:Value=sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'job_title',j.title,'job_code',j.job_code,'job_description',j.description,'state',s.state,'started_at',s.started_at,'submitted_at',s.submitted_at,'candidate_name',s.candidate_name,'candidate_email',s.candidate_email) FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.id=$1").bind(session).fetch_one(&mut *tx).await?;
    let questions:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',q.id,'position',q.position,'prompt',q.prompt,'rubric',q.rubric,'answer',a.content,'saved_at',a.updated_at) FROM interview_questions q LEFT JOIN interview_answers a ON a.question_id=q.id WHERE q.session_id=$1 ORDER BY q.position").bind(session).fetch_all(&mut *tx).await?;
    let events:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'at',created_at,'action',action,'actor_id',actor_id,'evidence',metadata) FROM audit_events WHERE entity_id=$1 AND action NOT IN ('reviewer.indexed','invitation.link_retrieved') ORDER BY created_at,id").bind(session).fetch_all(&mut *tx).await?;
    audit(
        &mut tx,
        owner,
        Some(owner),
        session,
        "report.exported",
        json!({"format":"json","version":1}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        json!({"version":1,"interview":interview,"questions":questions,"events":events,"integrity":integrity_report,"integrity_weights":weights(),"notice":"Scores and browser observations require human review. Identity signals must not influence scoring. Server signatures prove receipt integrity, not completeness or truth of browser telemetry."}),
    ))
}
pub(super) async fn analytics(
    State(pool): State<PgPool>,
    Path(job): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    let found: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM jobs WHERE id=$1)")
        .bind(job)
        .fetch_one(&mut *tx)
        .await?;
    if !found {
        return Err(ApiError(StatusCode::NOT_FOUND, "Job not found"));
    }
    let groups:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('kind',q.kind,'difficulty',q.difficulty,'competency_id',q.competency_id,'sample_size',count(*),'mean_points',avg((r->>'score')::numeric),'score_stddev',stddev_samp((r->>'score')::numeric),'item_total_correlation',corr((r->>'score')::double precision,((SELECT sum((p->>'score')::double precision) FROM jsonb_array_elements(a.metadata->'questions') p)-(r->>'score')::double precision))) FROM interview_questions q JOIN audit_events a ON a.entity_id=q.session_id AND a.action='assessment.completed' CROSS JOIN LATERAL jsonb_array_elements(a.metadata->'questions') r WHERE q.job_id=$1 AND r->>'question_id'=q.id::text GROUP BY q.kind,q.difficulty,q.competency_id").bind(job).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"groups":groups,"notice":"Descriptive statistics only. Unique questions and small samples cannot establish discrimination or fairness; validate with sufficient job-relevant outcomes."}),
    ))
}
pub(super) async fn metrics(State(pool): State<PgPool>, headers: HeaderMap) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    let metrics:Value=sqlx::query_scalar("WITH timings AS (SELECT a.entity_id,a.action,extract(epoch FROM (a.created_at-(SELECT min(b.created_at) FROM audit_events b WHERE b.entity_id=a.entity_id AND b.action=CASE WHEN a.action='assessment.completed' THEN 'assessment.started' ELSE 'questions.queued' END)))*1000 AS ms FROM audit_events a WHERE a.action IN ('assessment.completed','questions.prepared')) SELECT jsonb_build_object('assessment_p50_ms',(SELECT percentile_cont(0.5) WITHIN GROUP(ORDER BY ms) FROM timings WHERE action='assessment.completed'),'assessment_p99_ms',(SELECT percentile_cont(0.99) WITHIN GROUP(ORDER BY ms) FROM timings WHERE action='assessment.completed'),'generation_p50_ms',(SELECT percentile_cont(0.5) WITHIN GROUP(ORDER BY ms) FROM timings WHERE action='questions.prepared'),'generation_p99_ms',(SELECT percentile_cont(0.99) WITHIN GROUP(ORDER BY ms) FROM timings WHERE action='questions.prepared'),'sessions',(SELECT count(*) FROM interview_sessions),'observations',(SELECT count(*) FROM audit_events WHERE action='proctor.observed'),'scoring_failures',(SELECT count(*) FROM audit_events WHERE action='assessment.failed'),'observations_per_session',(SELECT count(*)::numeric/nullif((SELECT count(*) FROM interview_sessions),0) FROM audit_events WHERE action='proctor.observed'))").fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(metrics))
}
