use crate::{ApiError, Result, access::staff, generation::provider::AiProvider};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) fn routes() -> Router<PgPool> {
    Router::new().route("/api/v1/sessions/{session}/rag", get(status).post(retry))
}
async fn status(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    super::super::review::exists(&mut tx, session).await?;
    let job:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('status',status,'attempts',attempts,'revision',revision,'indexed_revision',indexed_revision,'last_error',last_error,'model',embedding_model,'dimensions',embedding_dimensions) FROM embedding_jobs WHERE session_id=$1")
        .bind(session).fetch_optional(&mut *tx).await?;
    let scored:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events WHERE entity_id=$1 AND action='assessment.completed')").bind(session).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    let mut response = job.unwrap_or_else(
        || json!({"status":if scored{"pending"}else{"awaiting_scoring"},"attempts":0}),
    );
    response["max_attempts"] = json!(super::worker::MAX_ATTEMPTS);
    response["configured"] = json!(AiProvider::from_environment().is_ok());
    Ok(Json(response))
}
async fn retry(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (owner, mut tx) = staff(&pool, &headers).await?;
    super::super::review::exists(&mut tx, session).await?;
    let scored:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events WHERE entity_id=$1 AND action='assessment.completed')").bind(session).fetch_one(&mut *tx).await?;
    if !scored {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Evidence indexing starts after scoring completes",
        ));
    }
    let affected=sqlx::query("INSERT INTO embedding_jobs(session_id,interviewer_id) VALUES($1,$2) ON CONFLICT(session_id) DO UPDATE SET status='pending',attempts=0,revision=embedding_jobs.revision+1,claim_token=NULL,lease_until=NULL,last_error=NULL,available_at=now(),updated_at=now() WHERE embedding_jobs.status<>'running' OR embedding_jobs.lease_until<now()")
        .bind(session).bind(owner).execute(&mut *tx).await?.rows_affected();
    if affected == 0 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Evidence indexing is already running",
        ));
    }
    crate::audit::record(&mut tx, owner, Some(owner), session, "rag.retry", json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({"status":"pending"})))
}
