//! Preparation status and manual retry orchestration. No HTTP extractors or DTOs.
use crate::{
    ApiError, Result,
    error::missing,
    infrastructure::interviewer_transaction,
    models::generation::{SessionStatus, SetStatus},
    repositories::generation_repository as repo,
};
use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) async fn session_status(
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
) -> Result<SessionStatus> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let row = repo::session(&mut tx, session)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "Session not found"))?;
    let questions = repo::session_questions(&mut tx, session).await?;
    tx.commit().await?;
    Ok(SessionStatus {
        row,
        questions,
    })
}
pub(crate) async fn set_status(pool: &PgPool, owner: Uuid, job: Uuid) -> Result<SetStatus> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let row = repo::job(&mut tx, job, false)
        .await?
        .ok_or_else(missing)?;
    let sets = repo::sets(&mut tx, job).await?;
    let questions = repo::questions(&mut tx, job).await?;
    tx.commit().await?;
    Ok(SetStatus {
        row,
        sets,
        questions,
    })
}
pub(crate) async fn retry(pool: &PgPool, owner: Uuid, job: Uuid, configured: bool) -> Result<()> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let row = repo::job(&mut tx, job, true)
        .await?
        .ok_or_else(missing)?;
    if !configured {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "AI generation is not configured",
        ));
    }
    repo::reset_failed(&mut tx, job).await?;
    crate::generation::bank::replenish(&mut tx, owner, job, row.preparation_duration_minutes)
        .await?;
    crate::audit::record(
        &mut tx,
        owner,
        Some(owner),
        job,
        "question_sets.retried",
        json!({"target":row.question_set_count}),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}
