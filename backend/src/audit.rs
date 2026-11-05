//! Durable domain events, separate from diagnostic logs.
use crate::{
    Result,
    access::staff,
    pagination::{Page, paginated},
};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn record(
    tx: &mut Transaction<'_, Postgres>,
    recruiter: Uuid,
    actor: Option<Uuid>,
    entity: Uuid,
    action: &str,
    metadata: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_events(id,interviewer_id,actor_id,entity_id,action,metadata) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(Uuid::new_v4()).bind(recruiter).bind(actor).bind(entity).bind(action).bind(metadata).execute(&mut **tx).await?;
    tracing::debug!(event="domain_event.staged", action, entity_id=%entity);
    Ok(())
}
pub(crate) async fn audit_log(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    let limit = page.limit()?;
    let (recruiter, mut tx) = staff(&pool, &headers).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(a) FROM audit_events a WHERE interviewer_id=$1 AND ($2::uuid IS NULL OR entity_id=$2) AND ($3::uuid IS NULL OR (created_at,id)<(SELECT created_at,id FROM audit_events WHERE id=$3)) ORDER BY created_at DESC,id DESC LIMIT $4")
        .bind(recruiter).bind(page.entity_id).bind(page.cursor).bind(limit+1).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(paginated(items, limit)))
}
