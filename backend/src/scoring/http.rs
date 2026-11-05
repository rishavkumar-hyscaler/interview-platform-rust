//! Assessment report and explicit retry endpoints.
use crate::{
    ApiError, Result, access::staff, audit::record as audit, generation::provider::AiProvider,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub fn routes() -> Router<PgPool> {
    Router::new()
        .route("/api/v1/sessions/{session}/assessment", get(report))
        .route("/api/v1/sessions/{session}/assessment/retry", post(retry))
}
async fn report(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    let state: String = sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1")
        .bind(session)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "Session not found"))?;
    let result: Option<Value> = sqlx::query_scalar("SELECT metadata FROM audit_events WHERE entity_id=$1 AND action='assessment.completed' ORDER BY created_at DESC LIMIT 1").bind(session).fetch_optional(&mut *tx).await?;
    let attempts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='assessment.started' AND created_at>coalesce((SELECT max(created_at) FROM audit_events WHERE entity_id=$1 AND action='assessment.retry'),'-infinity'::timestamptz)",
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await?;
    let observations: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',id,'received_at',created_at,'observation',metadata) FROM audit_events WHERE entity_id=$1 AND action='proctor.observed' ORDER BY created_at,id LIMIT 10000").bind(session).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"configured":AiProvider::from_environment().is_ok(),"state":state,"assessment":result,"attempts":attempts,"failed":result.is_none() && attempts>=3 && state=="submitted","observations":observations,"integrity_notice":"Browser observations are unverified signals, not proof of cheating. Human review required."}),
    ))
}
async fn retry(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (owner, mut tx) = staff(&pool, &headers).await?;
    let state: Option<String> =
        sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1 FOR UPDATE")
            .bind(session)
            .fetch_optional(&mut *tx)
            .await?;
    if !matches!(state.as_deref(), Some("submitted" | "scoring")) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Only unfinished assessments can be retried",
        ));
    }
    let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events WHERE entity_id=$1 AND action='assessment.started' AND created_at>now()-interval '15 minutes')").bind(session).fetch_one(&mut *tx).await?;
    if active {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Wait for the current assessment attempt to finish or expire",
        ));
    }
    audit(
        &mut tx,
        owner,
        Some(owner),
        session,
        "assessment.retry",
        json!({"reason":"interviewer_requested"}),
    )
    .await?;
    sqlx::query("UPDATE interview_sessions SET state='submitted' WHERE id=$1")
        .bind(session)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({"queued":true})))
}
