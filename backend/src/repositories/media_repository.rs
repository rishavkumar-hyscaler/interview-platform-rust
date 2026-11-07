//! Recording metadata persistence; file access belongs to the media service.
use crate::Result;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn recording(
    tx: &mut Transaction<'_, Postgres>,
    recording: Uuid,
    session: Uuid,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT metadata
        FROM audit_events
        WHERE id = $1
        AND entity_id = $2
        AND action = 'answer.recorded'
        "#,
    )
    .bind(recording)
    .bind(session)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn state(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1")
            .bind(session)
            .fetch_one(&mut **tx)
            .await?,
    )
}
pub(crate) async fn question_exists(
    tx: &mut Transaction<'_, Postgres>,
    question: Uuid,
    session: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM session_question_details()
            WHERE id = $1
            AND session_id = $2)
        "#,
    )
    .bind(question)
    .bind(session)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn used(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT count(*)
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'answer.recorded'
        "#,
    )
    .bind(session)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn active(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT state='in_progress' FROM interview_sessions WHERE id=$1")
            .bind(session)
            .fetch_one(&mut **tx)
            .await?,
    )
}
pub(crate) async fn list(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT metadata || jsonb_build_object('saved_at',
                created_at)
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'answer.recorded'
        ORDER BY created_at,
            id
        "#,
    )
    .bind(session)
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn session_exists(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM interview_sessions WHERE id=$1)")
            .bind(session)
            .fetch_one(&mut **tx)
            .await?,
    )
}
pub(crate) async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner: Uuid,
    session: Uuid,
    metadata: &Value,
) -> std::result::Result<(), sqlx::Error> {
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
                'answer.recorded',
                $4)
        "#,
    )
    .bind(id)
    .bind(owner)
    .bind(session)
    .bind(metadata)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
