//! Background persistence only. Services retain transaction and retry boundaries.
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgQueryResult};
use uuid::Uuid;

pub(crate) async fn interviewers(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer'")
        .fetch_all(pool)
        .await
}

pub(crate) async fn exhausted_session(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT s.id FROM interview_sessions s WHERE s.state='scoring' AND NOT EXISTS(SELECT 1 FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.started' AND a.created_at>now()-interval '15 minutes' AND NOT EXISTS(SELECT 1 FROM audit_events done WHERE done.entity_id=s.id AND done.action IN ('assessment.failed','assessment.completed') AND done.metadata->>'claim'=a.metadata->>'claim')) AND (SELECT count(*) FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.started' AND a.created_at>coalesce((SELECT max(r.created_at) FROM audit_events r WHERE r.entity_id=s.id AND r.action='assessment.retry'),'-infinity'::timestamptz))>=4 FOR UPDATE SKIP LOCKED LIMIT 1").fetch_optional(&mut **tx).await
}

pub(crate) async fn mark_submitted(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query("UPDATE interview_sessions SET state='submitted' WHERE id=$1")
        .bind(session)
        .execute(&mut **tx)
        .await
}

pub(crate) async fn next_session(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT s.id FROM interview_sessions s WHERE s.state IN ('submitted','scoring') AND NOT EXISTS(SELECT 1 FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.completed') AND (SELECT count(*) FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.started' AND a.created_at>coalesce((SELECT max(r.created_at) FROM audit_events r WHERE r.entity_id=s.id AND r.action='assessment.retry'),'-infinity'::timestamptz))<4 AND NOT EXISTS(SELECT 1 FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.started' AND a.created_at>now()-interval '15 minutes' AND NOT EXISTS(SELECT 1 FROM audit_events done WHERE done.entity_id=s.id AND done.action IN ('assessment.failed','assessment.completed') AND done.metadata->>'claim'=a.metadata->>'claim')) AND NOT EXISTS(SELECT 1 FROM audit_events f WHERE f.entity_id=s.id AND f.action='assessment.failed' AND f.created_at>now()-interval '15 seconds' AND f.created_at>coalesce((SELECT max(r.created_at) FROM audit_events r WHERE r.entity_id=s.id AND r.action='assessment.retry'),'-infinity'::timestamptz)) ORDER BY s.submitted_at FOR UPDATE SKIP LOCKED LIMIT 1").fetch_optional(&mut **tx).await
}

pub(crate) async fn mark_scoring(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query("UPDATE interview_sessions SET state='scoring' WHERE id=$1")
        .bind(session)
        .execute(&mut **tx)
        .await
}

pub(crate) async fn questions(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT jsonb_build_object('id',q.id,'position',q.position,'prompt',q.prompt,'rubric',q.rubric,'competency_id',q.competency_id,'answer',coalesce(a.content,''),'recordings',(SELECT count(*) FROM audit_events m WHERE m.entity_id=q.session_id AND m.action='answer.recorded' AND m.metadata->>'question_id'=q.id::text)) FROM session_question_details() q LEFT JOIN interview_answers a ON a.question_id=q.id AND a.session_id=q.session_id WHERE q.session_id=$1 ORDER BY q.position").bind(session).fetch_all(&mut **tx).await
}

pub(crate) async fn lock_session(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM interview_sessions WHERE id=$1 FOR UPDATE")
        .bind(session)
        .fetch_one(&mut **tx)
        .await
}

pub(crate) async fn current_claim(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Value, sqlx::Error> {
    sqlx::query_scalar("SELECT metadata FROM audit_events WHERE entity_id=$1 AND action='assessment.started' ORDER BY created_at DESC,id DESC LIMIT 1").bind(session).fetch_one(&mut **tx).await
}

pub(crate) async fn blueprint(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT j.blueprint FROM jobs j JOIN interview_sessions s ON s.job_id=j.id WHERE s.id=$1",
    )
    .bind(session)
    .fetch_one(&mut **tx)
    .await
}

pub(crate) async fn mark_completed(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query("UPDATE interview_sessions SET state='completed' WHERE id=$1")
        .bind(session)
        .execute(&mut **tx)
        .await
}

pub(crate) async fn checkpoint(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    fingerprint: &str,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT metadata->'result' FROM audit_events WHERE entity_id=$1 AND action='assessment.question.scored' AND metadata->>'input_hash'=$2 ORDER BY created_at DESC,id DESC LIMIT 1").bind(session).bind(fingerprint).fetch_optional(&mut **tx).await
}
