//! Durable events; no request parsing or API serialization.
use crate::{Result, models::audit::AuditEvent};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn record(
    tx: &mut Transaction<'_, Postgres>,
    recruiter: Uuid,
    actor: Option<Uuid>,
    entity: Uuid,
    action: &str,
    metadata: Value,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO audit_events(id,
                interviewer_id,
                actor_id,
                entity_id,
                action,
                metadata)
        VALUES ($1,
                $2,
                $3,
                $4,
                $5,
                $6)
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(recruiter)
    .bind(actor)
    .bind(entity)
    .bind(action)
    .bind(metadata)
    .execute(&mut **tx)
    .await?;
    tracing::debug!(event="domain_event.staged",action,entity_id=%entity);
    Ok(())
}
pub(crate) async fn list(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    entity: Option<Uuid>,
    cursor: Option<Uuid>,
    limit: i64,
) -> Result<Vec<AuditEvent>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id,
            interviewer_id,
            actor_id,
            entity_id,
            action,
            metadata,
            to_jsonb(created_at) #>> '{}' AS created_at
        FROM audit_events
        WHERE interviewer_id = $1
        AND ($2::uuid IS NULL
            OR entity_id = $2)
        AND ($3::uuid IS NULL
            OR (created_at,
                    id) < (
                SELECT created_at,
                    id
                FROM audit_events
                WHERE id = $3))
        ORDER BY created_at DESC,
            id DESC
        LIMIT $4
        "#,
    )
    .bind(owner)
    .bind(entity)
    .bind(cursor)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?)
}
