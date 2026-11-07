//! Observation persistence. Receipt signing and validation belong to the service.
use crate::Result;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn lock_session(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<(String, bool)> {
    Ok(sqlx::query_as(
        r#"
        SELECT state,
            expires_at > clock_timestamp()
        FROM interview_sessions
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn count(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT count(*)
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'proctor.observed'
        "#,
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn previous(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT metadata
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'proctor.observed'
        ORDER BY (metadata ->> 'sequence')::bigint DESC NULLS LAST,
            created_at DESC
        LIMIT 1
        "#,
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn duplicate(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    session: Uuid,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT metadata
        FROM audit_events
        WHERE id = $1
        AND entity_id = $2
        AND action = 'proctor.observed'
        "#,
    )
    .bind(event)
    .bind(session)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn received_at(tx: &mut Transaction<'_, Postgres>) -> Result<String> {
    Ok(sqlx::query_scalar("SELECT clock_timestamp()::text")
        .fetch_one(&mut **tx)
        .await?)
}
pub(crate) async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    owner: Uuid,
    session: Uuid,
    metadata: Value,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO audit_events(id,
                interviewer_id,
                entity_id,
                action,
                metadata)
        VALUES ($1,
                $2,
                $3,
                'proctor.observed',
                $4)
        "#,
    )
    .bind(event)
    .bind(owner)
    .bind(session)
    .bind(metadata)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
