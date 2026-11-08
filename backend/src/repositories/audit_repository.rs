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
        WITH page AS (
            SELECT *
            FROM audit_events
            WHERE interviewer_id = $1
              AND ($2::uuid IS NULL OR entity_id = $2)
              AND ($3::uuid IS NULL OR (created_at, id) < (
                  SELECT created_at, id
                  FROM audit_events
                  WHERE id = $3 AND interviewer_id = $1
              ))
            ORDER BY created_at DESC, id DESC
            LIMIT $4
        )
        SELECT a.id,
            a.interviewer_id,
            a.actor_id,
            a.entity_id,
            u.email AS actor_email,
            s.candidate_name,
            s.candidate_email,
            j.title AS job_title,
            j.job_code,
            q.position AS question_position,
            a.action,
            a.metadata,
            to_jsonb(a.created_at) #>> '{}' AS created_at
        FROM page a
        LEFT JOIN users u ON u.id = a.actor_id AND u.id = a.interviewer_id
        LEFT JOIN interview_sessions s
            ON s.id = a.entity_id AND s.interviewer_id = a.interviewer_id
        LEFT JOIN question_sets qs
            ON qs.id = a.entity_id AND qs.interviewer_id = a.interviewer_id
        LEFT JOIN jobs j
            ON j.id = coalesce(s.job_id, qs.job_id, a.entity_id)
            AND j.interviewer_id = a.interviewer_id
        LEFT JOIN interview_questions q
            ON q.id::text = a.metadata ->> 'question_id'
            AND q.interviewer_id = a.interviewer_id
            AND q.set_id = coalesce(s.question_set_id, qs.id)
        ORDER BY a.created_at DESC, a.id DESC
        "#,
    )
    .bind(owner)
    .bind(entity)
    .bind(cursor)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?)
}
