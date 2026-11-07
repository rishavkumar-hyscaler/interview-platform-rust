//! Assessment persistence, within caller-owned scoped transactions.
use crate::Result;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn state(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    lock: bool,
) -> Result<Option<String>> {
    let query = if lock {
        "SELECT state FROM interview_sessions WHERE id=$1 FOR UPDATE"
    } else {
        "SELECT state FROM interview_sessions WHERE id=$1"
    };
    Ok(sqlx::query_scalar(query)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?)
}
pub(crate) async fn assessment(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT metadata
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'assessment.completed'
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn attempts(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT count(*)
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'assessment.started'
        AND created_at > coalesce((
                SELECT max(created_at)
                FROM audit_events
                WHERE entity_id = $1
                AND action = 'assessment.retry'),
                '-infinity'::timestamptz)
        "#,
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn last_failure(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('reason',
                metadata ->> 'reason',
                'question_position',
                metadata -> 'question_position')
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'assessment.failed'
        AND created_at > coalesce((
                SELECT max(created_at)
                FROM audit_events
                WHERE entity_id = $1
                AND action = 'assessment.retry'),
                '-infinity'::timestamptz)
        ORDER BY created_at DESC,
            id DESC
        LIMIT 1
        "#,
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn observations(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                id,
                'received_at',
                created_at,
                'observation',
                metadata)
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'proctor.observed'
        ORDER BY created_at,
            id
        LIMIT 10000
        "#,
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn active(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM audit_events a
            WHERE a.entity_id = $1
            AND a.action = 'assessment.started'
            AND a.created_at > now() - interval '15 minutes'
            AND NOT EXISTS (
                SELECT 1
                FROM audit_events done
                WHERE done.entity_id = a.entity_id
                AND done.action IN ('assessment.failed',
                        'assessment.completed')
                AND done.metadata ->> 'claim' = a.metadata ->> 'claim'))
        "#,
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn queue(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<()> {
    sqlx::query("UPDATE interview_sessions SET state='submitted' WHERE id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
