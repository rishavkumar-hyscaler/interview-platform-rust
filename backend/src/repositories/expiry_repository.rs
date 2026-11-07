//! Background persistence only. Services retain transaction and retry boundaries.
use sqlx::{PgPool, Postgres, Transaction, postgres::PgQueryResult};
use uuid::Uuid;

pub(crate) async fn interviewers(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer' ORDER BY id")
        .fetch_all(pool)
        .await
}

pub(crate) async fn next_due_session(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM interview_sessions WHERE state IN ('invited','ready','in_progress') AND least(expires_at,coalesce(deadline_at,expires_at))<=now() ORDER BY expires_at,id FOR UPDATE SKIP LOCKED LIMIT 1").fetch_optional(&mut **tx).await
}

pub(crate) async fn lock_session(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM interview_sessions WHERE id=$1 FOR UPDATE")
        .bind(session)
        .fetch_optional(&mut **tx)
        .await
}

pub(crate) async fn finalize(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("UPDATE interview_sessions SET submitted_at=CASE WHEN state='in_progress' THEN least(expires_at,coalesce(deadline_at,expires_at)) ELSE submitted_at END,state=CASE WHEN state='in_progress' THEN 'submitted' ELSE 'expired' END WHERE id=$1 AND state IN ('invited','ready','in_progress') AND least(expires_at,coalesce(deadline_at,expires_at))<=clock_timestamp() RETURNING state").bind(session).fetch_optional(&mut **tx).await
}

pub(crate) async fn record_expiry(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    interviewer: Uuid,
    session: Uuid,
    action: &str,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,$4,'{\"actor\":\"system\",\"reason\":\"deadline\"}')").bind(id).bind(interviewer).bind(session).bind(action).execute(&mut **tx).await
}
