//! Browser observations are untrusted evidence, never cheating verdicts.
use crate::{ApiError, Result, access::candidate};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use ring::hmac;
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
    measurements: Option<Measurements>,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Measurements {
    duration_ms: u32,
    face_count: Option<u8>,
    speech_probability: Option<f32>,
    horizontal_offset: Option<f32>,
    vertical_offset: Option<f32>,
    mouth_motion: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head_horizontal_offset: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head_vertical_offset: Option<f32>,
}
impl Measurements {
    fn valid(&self) -> bool {
        self.duration_ms <= 120_000
            && self.face_count.is_none_or(|n| n <= 3)
            && self
                .speech_probability
                .is_none_or(|n| n.is_finite() && (0.0..=1.0).contains(&n))
            && [
                self.horizontal_offset,
                self.vertical_offset,
                self.mouth_motion,
                self.head_horizontal_offset,
                self.head_vertical_offset,
            ]
            .iter()
            .all(|n| n.is_none_or(|n| n.is_finite() && (-2.0..=2.0).contains(&n)))
    }
}
pub(crate) fn evidence_kind(kind: &str) -> bool {
    matches!(
        kind,
        "face_missing"
            | "multiple_faces"
            | "gaze_away"
            | "possible_background_speech"
            | "speech_detected"
    )
}
fn permission_event(kind: &str) -> bool {
    matches!(
        kind,
        "camera_permission_requested"
            | "microphone_permission_requested"
            | "camera_enabled"
            | "microphone_enabled"
            | "camera_denied"
            | "microphone_denied"
            | "camera_unavailable"
            | "microphone_unavailable"
            | "camera_disabled"
            | "microphone_disabled"
            | "screen_share_requested"
            | "screen_share_started"
            | "screen_share_denied"
            | "screen_share_unavailable"
            | "screen_share_stopped"
    )
}
pub(crate) fn screen_evidence_kind(kind: &str) -> bool {
    matches!(
        kind,
        "fullscreen_entered"
            | "fullscreen_exited"
            | "focus_lost"
            | "hidden"
            | "paste"
            | "copy"
            | "context_menu"
            | "devtools_suspected"
            | "multiple_displays"
            | "typing_burst"
            | "rapid_answer"
            | "camera_disabled"
            | "microphone_disabled"
            | "screen_share_started"
    ) || evidence_kind(kind)
}
fn valid(event: &Observation) -> bool {
    (matches!(
        event.kind.as_str(),
        "face_missing"
            | "multiple_faces"
            | "gaze_away"
            | "possible_background_speech"
            | "speech_detected"
            | "model_unavailable"
            | "face_analysis_ready"
            | "audio_analysis_ready"
            | "gaze_calibrated"
            | "monitoring_started"
            | "monitoring_stopped"
            | "camera_disabled"
            | "microphone_disabled"
            | "evidence_capture_failed"
            | "focus_lost"
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
            | "recording_denied"
            | "typing_burst"
            | "rapid_answer"
            | "multiple_displays"
            | "devtools_suspected"
            | "monitoring_capability"
    ) || permission_event(&event.kind))
        && event.measurements.as_ref().is_none_or(Measurements::valid)
        && (0..=4_102_444_800_000).contains(&event.client_time)
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
    let (state, active): (String, bool) = sqlx::query_as(
        "SELECT state,expires_at>clock_timestamp() FROM interview_sessions WHERE id=$1 FOR UPDATE",
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await?;
    let preflight = matches!(state.as_str(), "invited" | "ready") && active;
    if state != "in_progress"
        && !(preflight && events.iter().all(|event| permission_event(&event.kind)))
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Interview is not active; only permission checks can be recorded before starting",
        ));
    }
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='proctor.observed'",
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await?;
    let mut sequence = total;
    let previous:Option<Value>=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE entity_id=$1 AND action='proctor.observed' ORDER BY (metadata->>'sequence')::bigint DESC NULLS LAST,created_at DESC LIMIT 1").bind(session).fetch_optional(&mut *tx).await?;
    let mut previous_signature = previous
        .as_ref()
        .and_then(|v| v["signature"].as_str())
        .unwrap_or("legacy-or-genesis")
        .to_string();
    for event in events {
        let duplicate:Option<Value>=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='proctor.observed'").bind(event.id).bind(session).fetch_optional(&mut *tx).await?;
        if let Some(old) = duplicate {
            if old.get("measurements").unwrap_or(&Value::Null) != &json!(event.measurements)
                || old["kind"] != event.kind
                || old["client_time_ms"] != event.client_time
                || old.get("measurement").unwrap_or(&old["character_count"]) != &json!(event.count)
            {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "Event ID was already used for another observation",
                ));
            }
            continue;
        }
        if sequence >= 10000 {
            return Err(ApiError(
                StatusCode::TOO_MANY_REQUESTS,
                "Observation limit reached",
            ));
        }
        sequence += 1;
        let received: String = sqlx::query_scalar("SELECT clock_timestamp()::text")
            .fetch_one(&mut *tx)
            .await?;
        let mut metadata = json!({"source":"browser","kind":event.kind,"client_time_ms":event.client_time,"character_count":if matches!(event.kind.as_str(),"paste"|"typing_burst"){event.count}else{None},"measurement":event.count,"unit":match event.kind.as_str(){"paste"|"typing_burst"=>"characters","devtools_suspected"=>"outer_inner_pixels","rapid_answer"=>"seconds","multiple_displays"=>"minimum_displays",_=>"capability_boolean"},"classification":"observation","stage":if preflight {"preflight"}else{"interview"},"verified":false,"sequence":sequence,"previous_signature":previous_signature,"received_at":received});
        if let Some(measurements) = &event.measurements {
            metadata["measurements"] = json!(measurements);
        }
        let signature = sign(owner, session, event.id, &metadata).map_err(|_| {
            ApiError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Event signing is not configured",
            )
        })?;
        metadata["signature"] = json!(signature);
        sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'proctor.observed',$4)").bind(event.id).bind(owner).bind(session).bind(metadata).execute(&mut *tx).await?;
        previous_signature = signature;
    }
    tx.commit().await?;
    Ok(Json(json!({"accepted":true})))
}
pub(crate) fn sign(
    owner: Uuid,
    session: Uuid,
    id: Uuid,
    metadata: &Value,
) -> anyhow::Result<String> {
    let root = hex::decode(std::env::var("INVITATION_ENCRYPTION_KEY")?)?;
    anyhow::ensure!(root.len() == 32, "Invalid signing key");
    let derived = hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, &root),
        b"proctor-event-signing:v1",
    );
    let key = hmac::Key::new(hmac::HMAC_SHA256, derived.as_ref());
    let content = format!("v1:{owner}:{session}:{id}:{}", metadata);
    Ok(hex::encode(hmac::sign(&key, content.as_bytes()).as_ref()))
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
            measurements: None,
        };
        assert!(valid(&event));
        event.count = Some(-1);
        assert!(!valid(&event));
        event.count = None;
        event.kind = "cheating_confirmed".into();
        assert!(!valid(&event));
    }
    #[test]
    fn media_measurements_require_whole_milliseconds_and_health_events_are_valid() {
        let mut payload = json!({"id":Uuid::new_v4(),"kind":"face_missing","client_time":1,"measurements":{"duration_ms":5001.8,"face_count":0}});
        assert!(serde_json::from_value::<Observation>(payload.clone()).is_err());
        payload["measurements"]["duration_ms"] = json!(5001);
        let event: Observation = serde_json::from_value(payload).unwrap();
        assert!(valid(&event));
        for kind in [
            "face_analysis_ready",
            "audio_analysis_ready",
            "gaze_calibrated",
        ] {
            let event: Observation =
                serde_json::from_value(json!({"id":Uuid::new_v4(),"kind":kind,"client_time":1}))
                    .unwrap();
            assert!(valid(&event));
        }
    }
}
