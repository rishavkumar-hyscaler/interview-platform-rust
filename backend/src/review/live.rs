//! Live audit event streaming.
use crate::{ApiError, Result, access::staff};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Sse,
        sse::{Event, KeepAlive},
    },
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use std::{convert::Infallible, time::Duration};
use tokio_stream::wrappers::ReceiverStream;
use tracing::Instrument;
use uuid::Uuid;

use super::exists;
#[derive(Deserialize)]
pub(super) struct Cursor {
    after: Option<Uuid>,
}
pub(super) async fn live(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
    Query(cursor): Query<Cursor>,
) -> Result<impl IntoResponse> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    exists(&mut tx, session).await?;
    if let Some(after) = cursor.after {
        let valid: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM audit_events WHERE id=$1 AND entity_id=$2)",
        )
        .bind(after)
        .bind(session)
        .fetch_one(&mut *tx)
        .await?;
        if !valid {
            return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid event cursor"));
        }
    }
    tx.commit().await?;
    let (sender, receiver) =
        tokio::sync::mpsc::channel::<std::result::Result<Event, Infallible>>(16);
    let mut task = crate::observability::TaskLog::start("review.live", session);
    tokio::spawn(async move {
        let mut after = cursor.after;
        loop {
            if sender.is_closed() {
                break;
            }
            let Ok((_, mut tx)) = staff(&pool, &headers).await else {
                break;
            };
            let rows:std::result::Result<Vec<Value>,_>=sqlx::query_scalar("SELECT jsonb_build_object('id',id,'action',action,'created_at',created_at,'metadata',metadata) FROM audit_events WHERE entity_id=$1 AND action NOT IN ('reviewer.indexed','reviewer.message') AND ($2::uuid IS NULL OR (created_at,id)>(SELECT created_at,id FROM audit_events WHERE id=$2 AND entity_id=$1)) ORDER BY created_at,id LIMIT 100").bind(session).bind(after).fetch_all(&mut *tx).await;
            let _ = tx.commit().await;
            let Ok(rows) = rows else {
                break;
            };
            let full = rows.len() == 100;
            for row in rows {
                after = row["id"].as_str().and_then(|s| s.parse().ok());
                let event = Event::default()
                    .event("audit")
                    .id(after.map(|v| v.to_string()).unwrap_or_default())
                    .data(row.to_string());
                if sender.send(Ok(event)).await.is_err() {
                    return;
                }
            }
            if !full {
                tokio::select! {_=sender.closed()=>break,_=tokio::time::sleep(Duration::from_secs(2))=>{}}
            }
        }
        task.finish(sender.is_closed());
    }.in_current_span());
    Ok(Sse::new(ReceiverStream::new(receiver))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10))))
}
