//! Post-scoring indexing rules and scoped transaction ownership.
use crate::{
    ApiError, Result, infrastructure::interviewer_transaction, models::rag::IndexStatus,
    repositories::rag_repository as repo,
};
use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) async fn status(pool: &PgPool, owner: Uuid, session: Uuid) -> Result<IndexStatus> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    if !repo::exists(&mut tx, session).await? {
        return Err(ApiError(StatusCode::NOT_FOUND, "Session not found"));
    }
    let job = repo::job(&mut tx, session).await?;
    let scored = repo::scored(&mut tx, session).await?;
    tx.commit().await?;
    Ok(IndexStatus { job, scored })
}
pub(crate) async fn retry(pool: &PgPool, owner: Uuid, session: Uuid) -> Result<()> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    if !repo::exists(&mut tx, session).await? {
        return Err(ApiError(StatusCode::NOT_FOUND, "Session not found"));
    }
    if !repo::scored(&mut tx, session).await? {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Evidence indexing starts after scoring completes",
        ));
    }
    if !repo::queue(&mut tx, session, owner).await? {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Evidence indexing is already running",
        ));
    }
    crate::audit::record(&mut tx, owner, Some(owner), session, "rag.retry", json!({})).await?;
    tx.commit().await?;
    Ok(())
}
