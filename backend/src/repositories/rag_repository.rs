//! Embedding job inspection and retry persistence.
use crate::{Result, models::rag::IndexJob};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn exists(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM interview_sessions WHERE id=$1)")
            .bind(session)
            .fetch_one(&mut **tx)
            .await?,
    )
}
pub(crate) async fn job(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Option<IndexJob>> {
    Ok(sqlx::query_as(
        r#"
        SELECT status,
            attempts,
            revision,
            indexed_revision,
            last_error,
            embedding_model AS model,
            embedding_dimensions AS dimensions
        FROM embedding_jobs
        WHERE session_id = $1
        "#,
    )
    .bind(session)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn scored(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<bool> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM audit_events
            WHERE entity_id = $1
            AND action = 'assessment.completed')
        "#,
    )
    .bind(session)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn queue(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    owner: Uuid,
) -> Result<bool> {
    Ok(sqlx::query(
        r#"
        INSERT INTO embedding_jobs(session_id,
                interviewer_id)
        VALUES ($1,
                $2)
        ON CONFLICT (session_id)
        DO UPDATE
        SET status = 'pending',
            attempts = 0,
            revision = embedding_jobs.revision + 1,
            claim_token = NULL,
            lease_until = NULL,
            last_error = NULL,
            available_at = now(),
            updated_at = now()
        WHERE embedding_jobs.status <> 'running'
        OR embedding_jobs.lease_until < now()
        "#,
    )
    .bind(session)
    .bind(owner)
    .execute(&mut **tx)
    .await?
    .rows_affected()
        != 0)
}
