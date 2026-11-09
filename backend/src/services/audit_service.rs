use crate::{
    Result,
    error::bad,
    infrastructure::interviewer_transaction,
    models::audit::{AuditEvent, AuditFilters},
    repositories::audit_repository,
};
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) struct AuditPage {
    pub items: Vec<AuditEvent>,
    pub next_cursor: Option<Uuid>,
    pub total: i64,
}
pub(crate) async fn list(
    pool: &PgPool,
    owner: Uuid,
    entity: Option<Uuid>,
    cursor: Option<Uuid>,
    limit: i64,
    filters: &AuditFilters,
) -> Result<AuditPage> {
    if !(1..=100).contains(&limit) {
        return Err(bad("Limit must be between 1 and 100"));
    }
    if filters
        .from
        .zip(filters.until)
        .is_some_and(|(from, until)| from >= until)
    {
        return Err(bad("Start date must be on or before the end date"));
    }
    let mut tx = interviewer_transaction(pool, owner).await?;
    let mut items =
        audit_repository::list(&mut tx, owner, entity, cursor, limit + 1, filters).await?;
    let total = audit_repository::count(&mut tx, owner, entity, filters).await?;
    tx.commit().await?;
    let more = items.len() > limit as usize;
    items.truncate(limit as usize);
    let next_cursor = if more {
        items.last().map(|event| event.id)
    } else {
        None
    };
    Ok(AuditPage {
        items,
        next_cursor,
        total,
    })
}
