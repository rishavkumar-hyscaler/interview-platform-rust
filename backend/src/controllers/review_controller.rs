//! Review HTTP contracts and SSE framing. Services own evidence/AI workflows.
use crate::{
    ApiError, Result, authenticate,
    dto::review::*,
    services::{
        review_chat_service::{self, Chat, StreamEvent},
        review_service,
    },
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{convert::Infallible, time::Duration};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tracing::Instrument;
use uuid::Uuid;
pub(crate) fn routes() -> Router<PgPool> {
    Router::new()
        .route("/api/v1/sessions/{session}/live", get(live))
        .route("/api/v1/sessions/{session}/report", get(export_report))
        .route("/api/v1/jobs/{job}/analytics", get(analytics))
        .route("/api/v1/metrics", get(metrics))
        .route("/api/v1/sessions/{session}/integrity", get(integrity))
        .route(
            "/api/v1/sessions/{session}/integrity/summary",
            get(integrity_summary),
        )
        .route("/api/v1/sessions/{session}/annotations", post(annotate))
        .route("/api/v1/sessions/{session}/review", post(chat))
}
async fn integrity(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<IntegrityResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    response(review_service::integrity(&pool, owner, session).await?)
}
async fn integrity_summary(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<IntegritySummaryResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    response(review_service::integrity_summary(&pool, owner, session).await?)
}
async fn annotate(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<AnnotationRequest>,
) -> Result<Json<SavedResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    review_service::annotate(
        &pool,
        owner,
        session,
        review_service::Annotation {
            event_id: input.event_id,
            decision: input.decision,
            note: input.note,
        },
    )
    .await?;
    Ok(Json(SavedResponse {
        saved: true,
    }))
}
async fn export_report(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<ExportResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    response(review_service::export_report(&pool, owner, session).await?)
}
async fn analytics(
    State(pool): State<PgPool>,
    Path(job): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<AnalyticsResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    response(review_service::analytics(&pool, owner, job).await?)
}
async fn metrics(State(pool): State<PgPool>, headers: HeaderMap) -> Result<Json<MetricsResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    response(review_service::metrics(&pool, owner).await?)
}
fn response<T: DeserializeOwned>(value: Value) -> Result<Json<T>> {
    serde_json::from_value(value)
        .map(Json)
        .map_err(|_| ApiError(StatusCode::INTERNAL_SERVER_ERROR, "Review data unavailable"))
}
async fn chat(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<ChatRequest>,
) -> Result<impl IntoResponse> {
    let (owner, session_hash) = authenticate(&pool, &headers).await?;
    review_service::validate_live(&pool, owner, session, None).await?;
    let (sender, receiver) = tokio::sync::mpsc::channel::<StreamEvent>(64);
    let mut task = crate::observability::TaskLog::start("review.chat", session);
    tokio::spawn(
        async move {
            let result = review_chat_service::chat_answer(
                pool,
                session,
                owner,
                session_hash,
                Chat {
                    question: input.question,
                    conversation: input.conversation,
                    page: input.page,
                },
                &sender,
            )
            .await;
            let success = result.is_ok();
            let event = match result {
                Ok(value) => StreamEvent::Complete(value),
                Err(error) => StreamEvent::Failure(error.1),
            };
            let delivered = sender.send(event).await.is_ok();
            task.finish(success && delivered);
        }
        .in_current_span(),
    );
    let stream = ReceiverStream::new(receiver).map(|event| {
        Ok::<_, Infallible>(match event {
            StreamEvent::Delta(delta) => Event::default()
                .event("delta")
                .data(json!({"delta":delta,"verified":false}).to_string()),
            StreamEvent::Complete(value) => Event::default()
                .event("complete")
                .data(value.to_string()),
            StreamEvent::Failure(error) => Event::default()
                .event("failure")
                .data(json!({"error":error}).to_string()),
        })
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(10))))
}
async fn live(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
    Query(cursor): Query<LiveQuery>,
) -> Result<impl IntoResponse> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    review_service::validate_live(&pool, owner, session, cursor.after).await?;
    let (sender, receiver) =
        tokio::sync::mpsc::channel::<std::result::Result<Event, Infallible>>(16);
    let mut task = crate::observability::TaskLog::start("review.live", session);
    tokio::spawn(async move {
  let mut after=cursor.after;
  loop {
   if sender.is_closed(){break;}
   let Ok((owner,_))=authenticate(&pool,&headers).await else{break;};
   let Ok(rows)=review_service::live_page(&pool,owner,session,after).await else{break;};
   let full=rows.len()==100;
   for row in rows {
    after=row["id"].as_str().and_then(|v|v.parse().ok());
    let event=Event::default().event("audit").id(after.map(|v|v.to_string()).unwrap_or_default()).data(row.to_string());
    if sender.send(Ok(event)).await.is_err(){return;}
   }
   if !full {tokio::select!{_=sender.closed()=>break,_=tokio::time::sleep(Duration::from_secs(2))=>{}}}
  }
  task.finish(sender.is_closed());
 }.in_current_span());
    Ok(Sse::new(ReceiverStream::new(receiver))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10))))
}
