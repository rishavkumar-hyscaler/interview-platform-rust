//! Browser observation HTTP input/output only.
use crate::{
    Result,
    access::candidate,
    dto::proctoring::{AcceptedResponse, ObservationRequest},
    models::proctoring::{Measurements, Observation},
    services::proctoring_service,
};
use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use sqlx::PgPool;
pub(crate) fn routes() -> Router<PgPool> {
    Router::new().route("/api/v1/candidate/events", post(record))
}
async fn record(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(events): Json<Vec<ObservationRequest>>,
) -> Result<Json<AcceptedResponse>> {
    let (owner, session, tx) = candidate(&pool, &headers).await?;
    let events = events
        .into_iter()
        .map(|event| Observation {
            id: event.id,
            kind: event.kind,
            client_time: event.client_time,
            count: event.count,
            measurements: event
                .measurements
                .map(|m| Measurements {
                    duration_ms: m.duration_ms,
                    face_count: m.face_count,
                    speech_probability: m.speech_probability,
                    horizontal_offset: m.horizontal_offset,
                    vertical_offset: m.vertical_offset,
                    mouth_motion: m.mouth_motion,
                    head_horizontal_offset: m.head_horizontal_offset,
                    head_vertical_offset: m.head_vertical_offset,
                }),
        })
        .collect();
    proctoring_service::record(owner, session, tx, events).await?;
    Ok(Json(AcceptedResponse {
        accepted: true,
    }))
}
