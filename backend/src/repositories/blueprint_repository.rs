//! Durable blueprint state lives in append-only job audit events.
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct BlueprintJob {
    pub id: Uuid,
    pub description: String,
    pub duration: i32,
    pub preparation: Option<Value>,
}

pub(crate) async fn claimable(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Option<BlueprintJob>, sqlx::Error> {
    sqlx::query_as(r#"
        SELECT j.id, j.description, j.preparation_duration_minutes AS duration,
            task.metadata AS preparation
        FROM jobs j
        LEFT JOIN LATERAL (
            SELECT metadata FROM audit_events
            WHERE entity_id=j.id AND action='job.blueprint_task'
            ORDER BY created_at DESC,id DESC LIMIT 1
        ) task ON true
        WHERE j.blueprint IS NULL AND task.metadata IS NOT NULL
        AND (coalesce((task.metadata->>'attempts')::int,0) < 4
            OR (task.metadata->>'status'='running' AND (task.metadata->>'lease_until')::timestamptz<now()))
        AND (
            (task.metadata->>'status'='pending'
                AND coalesce((task.metadata->>'available_at')::timestamptz,now())<=now())
            OR (task.metadata->>'status'='running'
                AND (task.metadata->>'lease_until')::timestamptz<now())
        )
        ORDER BY j.created_at,j.id
        FOR UPDATE OF j SKIP LOCKED LIMIT 1
    "#).fetch_optional(&mut **tx).await
}

pub(crate) async fn lock(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
) -> Result<Option<bool>, sqlx::Error> {
    sqlx::query_scalar("SELECT blueprint IS NOT NULL FROM jobs WHERE id=$1 FOR UPDATE")
        .bind(job)
        .fetch_optional(&mut **tx)
        .await
}

pub(crate) async fn state(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT metadata FROM audit_events WHERE entity_id=$1 AND action='job.blueprint_task' ORDER BY created_at DESC,id DESC LIMIT 1")
        .bind(job).fetch_optional(&mut **tx).await
}

pub(crate) async fn deadline(
    tx: &mut Transaction<'_, Postgres>,
    seconds: i32,
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar("SELECT (clock_timestamp()+make_interval(secs => $1::int))::text")
        .bind(seconds)
        .fetch_one(&mut **tx)
        .await
}
