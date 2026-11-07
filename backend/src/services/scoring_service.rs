//! Assessment inspection and retry rules. Workers retain their durable claim pipeline.
use crate::{
    ApiError, Result, infrastructure::interviewer_transaction, models::scoring::AssessmentReport,
    repositories::scoring_repository as repo,
};
use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) async fn report(pool: &PgPool, owner: Uuid, session: Uuid) -> Result<AssessmentReport> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let state = repo::state(&mut tx, session, false)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "Session not found"))?;
    let assessment = repo::assessment(&mut tx, session).await?;
    let attempts = repo::attempts(&mut tx, session).await?;
    let last_failure = repo::last_failure(&mut tx, session).await?;
    let observations = repo::observations(&mut tx, session).await?;
    tx.commit().await?;
    Ok(AssessmentReport {
        state,
        assessment,
        attempts,
        last_failure,
        observations,
    })
}
pub(crate) async fn retry(pool: &PgPool, owner: Uuid, session: Uuid) -> Result<()> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let state = repo::state(&mut tx, session, true).await?;
    if !matches!(state.as_deref(), Some("submitted" | "scoring")) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Only unfinished assessments can be retried",
        ));
    }
    if repo::active(&mut tx, session).await? {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Wait for the current assessment attempt to finish or expire",
        ));
    }
    crate::audit::record(
        &mut tx,
        owner,
        Some(owner),
        session,
        "assessment.retry",
        json!({"reason":"interviewer_requested"}),
    )
    .await?;
    repo::queue(&mut tx, session).await?;
    tx.commit().await?;
    Ok(())
}
