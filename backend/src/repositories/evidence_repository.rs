//! Evidence audit references. File integrity and signing stay in the service.
use crate::Result;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn lock_session(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<()> {
    sqlx::query("SELECT id FROM interview_sessions WHERE id=$1 FOR UPDATE")
        .bind(session)
        .fetch_one(&mut **tx)
        .await?;
    Ok(())
}
pub(crate) async fn existing(
    tx: &mut Transaction<'_, Postgres>,
    asset: Uuid,
    session: Uuid,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar("SELECT metadata FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='proctor.evidence'").bind(asset).bind(session).fetch_optional(&mut **tx).await?)
}
pub(crate) async fn observed(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    session: Uuid,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar("SELECT metadata FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='proctor.observed' AND created_at>clock_timestamp()-interval '15 minutes'").bind(event).bind(session).fetch_optional(&mut **tx).await?)
}
pub(crate) async fn count(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    source: &str,
) -> Result<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='proctor.evidence' AND coalesce(metadata->>'evidence_source','camera')=$2").bind(session).bind(source).fetch_one(&mut **tx).await?)
}
pub(crate) async fn same_kind(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    event: Uuid,
    image: bool,
    source: &str,
) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events WHERE entity_id=$1 AND action='proctor.evidence' AND metadata->>'event_id'=$2 AND (metadata->>'mime' LIKE 'image/%')=$3 AND coalesce(metadata->>'evidence_source','camera')=$4)").bind(session).bind(event.to_string()).bind(image).bind(source).fetch_one(&mut **tx).await?)
}
pub(crate) async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    asset: Uuid,
    owner: Uuid,
    session: Uuid,
    metadata: &Value,
) -> std::result::Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'proctor.evidence',$4)").bind(asset).bind(owner).bind(session).bind(metadata).execute(&mut **tx).await?;
    Ok(())
}
