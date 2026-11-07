//! Session reads and pagination in interviewer-scoped transactions.
use crate::{
    Result,
    error::{bad, missing},
    infrastructure::interviewer_transaction,
    models::interview::{SessionDetail, SessionListItem, Summary},
    repositories::interview_repository as repo,
};
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) struct SessionPage {
    pub items: Vec<SessionListItem>,
    pub next_cursor: Option<Uuid>,
}
pub(crate) async fn list(
    pool: &PgPool,
    owner: Uuid,
    cursor: Option<Uuid>,
    limit: i64,
) -> Result<SessionPage> {
    if !(1..=100).contains(&limit) {
        return Err(bad("Limit must be between 1 and 100"));
    }
    let mut tx = interviewer_transaction(pool, owner).await?;
    let mut items = repo::list(&mut tx, owner, cursor, limit + 1).await?;
    tx.commit().await?;
    let more = items.len() > limit as usize;
    items.truncate(limit as usize);
    let next_cursor = if more {
        items.last().map(|item| item.session.id)
    } else {
        None
    };
    Ok(SessionPage { items, next_cursor })
}
pub(crate) async fn detail(pool: &PgPool, owner: Uuid, id: Uuid) -> Result<SessionDetail> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let result = repo::detail(&mut tx, owner, id)
        .await?
        .ok_or_else(missing)?;
    tx.commit().await?;
    Ok(result)
}
pub(crate) async fn summary(pool: &PgPool, owner: Uuid) -> Result<Summary> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let result = repo::summary(&mut tx).await?;
    tx.commit().await?;
    Ok(result)
}
