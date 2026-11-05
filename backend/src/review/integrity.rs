//! Evidence weights, annotations and integrity summaries.
use crate::{ApiError, Result, access::staff, audit::record as audit};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use super::exists;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Annotation {
    event_id: Uuid,
    decision: String,
    note: String,
}
pub(super) async fn annotate(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<Annotation>,
) -> Result<Json<Value>> {
    if !matches!(
        input.decision.as_str(),
        "dismissed" | "suspected" | "confirmed_activity"
    ) || input.note.trim().is_empty()
        || input.note.len() > 2000
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Provide a decision and a note up to 2000 bytes",
        ));
    }
    let (owner, mut tx) = staff(&pool, &headers).await?;
    exists(&mut tx, session).await?;
    let found:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM audit_events WHERE id=$1 AND entity_id=$2 AND action='proctor.observed')").bind(input.event_id).bind(session).fetch_one(&mut *tx).await?;
    if !found {
        return Err(ApiError(StatusCode::NOT_FOUND, "Observation not found"));
    }
    audit(
        &mut tx,
        owner,
        Some(owner),
        session,
        "integrity.annotated",
        json!({"event_id":input.event_id,"decision":input.decision,"note":input.note.trim()}),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"saved":true})))
}
pub(super) fn weights() -> Value {
    let defaults = json!({"focus_lost":2,"hidden":2,"fullscreen_exited":3,"paste":2,"context_menu":1,"camera_denied":0,"microphone_denied":0,"typing_burst":3,"rapid_answer":2,"multiple_displays":2,"devtools_suspected":1});
    let mut result = defaults;
    if let Ok(raw) = std::env::var("INTEGRITY_WEIGHTS")
        && let Ok(Value::Object(custom)) = serde_json::from_str::<Value>(&raw)
    {
        for (k, v) in custom {
            if result.get(&k).is_some() && v.as_u64().is_some_and(|n| n <= 20) {
                result[k] = v;
            }
        }
    }
    result
}
pub(super) async fn integrity(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (owner, mut tx) = staff(&pool, &headers).await?;
    exists(&mut tx, session).await?;
    let mut events:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',a.id,'received_at',a.created_at,'evidence',a.metadata,'annotation',(SELECT jsonb_build_object('reviewer',b.actor_id,'at',b.created_at,'decision',b.metadata->>'decision','note',b.metadata->>'note') FROM audit_events b WHERE b.entity_id=a.entity_id AND b.action='integrity.annotated' AND b.metadata->>'event_id'=a.id::text ORDER BY b.created_at DESC,b.id DESC LIMIT 1)) FROM audit_events a WHERE entity_id=$1 AND action='proctor.observed' ORDER BY created_at,id LIMIT 10000").bind(session).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let mut prior = String::from("legacy-or-genesis");
    let mut expected_sequence = 0i64;
    for event in &mut events {
        expected_sequence += 1;
        let mut metadata = event["evidence"].clone();
        let signature = metadata.as_object_mut().and_then(|v| v.remove("signature"));
        let id = event["id"]
            .as_str()
            .and_then(|v| v.parse::<Uuid>().ok())
            .unwrap_or_default();
        let status = if let Some(signature) = signature {
            let valid = metadata["sequence"] == expected_sequence
                && metadata["previous_signature"] == prior
                && crate::proctoring::sign(owner, session, id, &metadata)
                    .ok()
                    .as_deref()
                    == signature.as_str();
            prior = signature.as_str().unwrap_or("").into();
            if valid {
                "valid_server_receipt"
            } else {
                "invalid"
            }
        } else {
            "legacy_unsigned"
        };
        event["chain_status"] = json!(status);
    }
    let weights = weights();
    let mut score = 0u64;
    for e in &events {
        if e["annotation"]["decision"] != "dismissed" {
            score += weights[e["evidence"]["kind"].as_str().unwrap_or("")]
                .as_u64()
                .unwrap_or(0);
        }
    }
    Ok(Json(
        json!({"risk_score":score.min(100),"weights":weights,"events":events,"summary":format!("{} browser observations; {} weighted risk points. Review the evidence and context before drawing conclusions.",events.len(),score.min(100)),"notice":"Risk points are review priorities, not a probability or verdict of cheating. Confirmed activity means a reviewer confirmed that activity occurred, not cheating."}),
    ))
}
