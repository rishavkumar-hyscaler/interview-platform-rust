use crate::{
    Result, authenticate,
    dto::scoring::{AssessmentResponse, RetryResponse},
    generation::provider::AiProvider,
    services::scoring_service,
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::HeaderMap,
    response::{
        IntoResponse, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use serde_json::json;
use sqlx::PgPool;
use std::{convert::Infallible, time::Duration};
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;
pub fn routes() -> Router<PgPool> {
    Router::new()
        .route("/api/v1/sessions/{session}/assessment", get(report))
        .route("/api/v1/sessions/{session}/assessment/retry", post(retry))
        .route("/api/v1/sessions/{session}/assessment/stream", get(stream))
}
async fn report(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<AssessmentResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    let r = scoring_service::report(&pool, owner, session).await?;
    let failed = r.assessment.is_none() && r.attempts >= 4 && r.state == "submitted";
    Ok(Json(AssessmentResponse {
        configured: AiProvider::from_environment().is_ok(),
        state: r.state,
        assessment: r.assessment,
        attempts: r.attempts,
        max_attempts: 4,
        last_failure: r.last_failure,
        failed,
        observations: r.observations,
        integrity_notice: "Browser observations are unverified signals, not proof of cheating. Human review required.",
    }))
}
async fn retry(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<RetryResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    scoring_service::retry(&pool, owner, session).await?;
    Ok(Json(RetryResponse {
        queued: true,
    }))
}
/// Live, unverified scoring rationale while the worker grades each answer. Events:
/// `rationale` (partial criteria), `scored` (validated and saved), `rejected`, `finished`.
/// The feed is in-process; clients still reload `/assessment` for authoritative scores.
async fn stream(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<impl IntoResponse> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    scoring_service::state(&pool, owner, session).await?;
    struct Release(Uuid);
    impl Drop for Release {
        fn drop(&mut self) {
            crate::scoring::live::release(self.0);
        }
    }
    let (snapshot, mut receiver) = crate::scoring::live::subscribe(session);
    let events = async_stream::stream! {
        let _release = Release(session);
        let event = |value: serde_json::Value| {
            let kind = value["type"].as_str().unwrap_or("rationale").to_owned();
            Ok::<_, Infallible>(Event::default().event(kind).data(value.to_string()))
        };
        if let Some(snapshot) = snapshot {
            yield event(snapshot);
        }
        loop {
            tokio::select! {
                received = receiver.recv() => match received {
                    Ok(value) => yield event(value),
                    // Snapshots are cumulative, so skipped ones lose nothing.
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                },
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    // Re-check access, and end once no scoring can still happen.
                    let Ok((owner, _)) = authenticate(&pool, &headers).await else { break };
                    match scoring_service::state(&pool, owner, session).await {
                        Ok(state) if matches!(state.as_str(), "submitted" | "scoring") => {}
                        _ => {
                            yield event(json!({"type":"finished","success":true}));
                            break;
                        }
                    }
                }
            }
        }
    };
    Ok(Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))))
}
