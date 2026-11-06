//! Review route composition and shared session ownership check.
mod assistant;
mod citations;
mod integrity;
mod live;
mod performance;
mod reports;
use crate::{ApiError, Result};
use assistant::chat;
use axum::{
    Router,
    http::StatusCode,
    routing::{get, post},
};
use integrity::{annotate, integrity};
use live::live;
use reports::{analytics, export_report, metrics};
use sqlx::PgPool;
use uuid::Uuid;
pub fn routes() -> Router<PgPool> {
    Router::new()
        .route("/api/v1/sessions/{session}/live", get(live))
        .route("/api/v1/sessions/{session}/report", get(export_report))
        .route("/api/v1/jobs/{job}/analytics", get(analytics))
        .route("/api/v1/metrics", get(metrics))
        .route("/api/v1/sessions/{session}/integrity", get(integrity))
        .route("/api/v1/sessions/{session}/annotations", post(annotate))
        .route("/api/v1/sessions/{session}/review", post(chat))
}
pub(crate) async fn exists(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    session: Uuid,
) -> Result<()> {
    let found: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM interview_sessions WHERE id=$1)")
            .bind(session)
            .fetch_one(&mut **tx)
            .await?;
    if !found {
        return Err(ApiError(StatusCode::NOT_FOUND, "Session not found"));
    }
    Ok(())
}
