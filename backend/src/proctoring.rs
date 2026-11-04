//! Browser observations are untrusted evidence, never cheating verdicts.
use crate::{ApiError, Result, interviews::candidate};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub fn routes() -> Router<PgPool> {
    Router::new().route("/api/v1/candidate/events", post(record))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    id: Uuid,
    kind: String,
    client_time: i64,
    count: Option<i32>,
}
fn valid(event: &Observation) -> bool {
    matches!(
        event.kind.as_str(),
        "focus_lost"
            | "focus_returned"
            | "hidden"
            | "visible"
            | "fullscreen_entered"
            | "fullscreen_exited"
            | "copy"
            | "paste"
            | "context_menu"
            | "camera_enabled"
            | "camera_denied"
            | "microphone_enabled"
            | "microphone_denied"
    ) && (0..=4_102_444_800_000).contains(&event.client_time)
        && event
            .count
            .is_none_or(|count| (0..=1_000_000).contains(&count))
}
async fn record(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(events): Json<Vec<Observation>>,
) -> Result<Json<Value>> {
    if events.is_empty() || events.len() > 40 || events.iter().any(|event| !valid(event)) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid browser observations",
        ));
    }
    let (owner, session, mut tx) = candidate(&pool, &headers).await?;
    let state: String =
        sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1 FOR UPDATE")
            .bind(session)
            .fetch_one(&mut *tx)
            .await?;
    if state != "in_progress" {
        return Err(ApiError(StatusCode::CONFLICT, "Interview is not active"));
    }
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='proctor.observed'",
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await?;
    if total + events.len() as i64 > 10000 {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "Observation limit reached",
        ));
    }
    for event in events {
        sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'proctor.observed',$4) ON CONFLICT(id) DO NOTHING")
            .bind(event.id).bind(owner).bind(session).bind(json!({"source":"browser","kind":event.kind,"client_time_ms":event.client_time,"character_count":event.count,"classification":"observation","verified":false})).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(Json(json!({"accepted":true})))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restrict_observation_payload() {
        let mut event = Observation {
            id: Uuid::new_v4(),
            kind: "paste".into(),
            client_time: 1,
            count: Some(100),
        };
        assert!(valid(&event));
        event.count = Some(-1);
        assert!(!valid(&event));
        event.count = None;
        event.kind = "cheating_confirmed".into();
        assert!(!valid(&event));
    }
}
