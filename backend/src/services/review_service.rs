//! Evidence review rules, signed receipt verification, annotations and reports.
use crate::{
    ApiError, Result, audit::record as audit, infrastructure::interviewer_transaction,
    repositories::review_repository as repo,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
pub(crate) struct Annotation {
    pub event_id: Uuid,
    pub decision: String,
    pub note: String,
}
pub(crate) async fn exists(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<()> {
    if !repo::session_exists(tx, session).await? {
        return Err(ApiError(StatusCode::NOT_FOUND, "Session not found"));
    }
    Ok(())
}
pub(crate) async fn annotate(
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
    input: Annotation,
) -> Result<()> {
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
    let mut tx = interviewer_transaction(pool, owner).await?;
    exists(&mut tx, session).await?;
    let found = repo::observation_exists(&mut tx, input.event_id, session).await?;
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
    Ok(())
}
pub(crate) fn weights() -> Value {
    let defaults = json!({"focus_lost":2,"hidden":2,"fullscreen_exited":3,"paste":2,"context_menu":1,"camera_denied":0,"microphone_denied":0,"typing_burst":3,"rapid_answer":2,"multiple_displays":2,"devtools_suspected":1,"face_missing":1,"multiple_faces":4,"gaze_away":0,"possible_background_speech":1,"speech_detected":0,"copy_from_question":0,"paste_from_question":0,"question_context_menu":0});
    let mut result = defaults;
    if let Ok(raw) = std::env::var("INTEGRITY_WEIGHTS")
        && let Ok(Value::Object(custom)) = serde_json::from_str::<Value>(&raw)
    {
        for (k, v) in custom {
            if !crate::proctoring::permitted_question_action(&k)
                && result.get(&k).is_some()
                && v.as_u64().is_some_and(|n| n <= 20)
            {
                result[k] = v;
            }
        }
    }
    result
}
pub(crate) async fn integrity(pool: &PgPool, owner: Uuid, session: Uuid) -> Result<Value> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    exists(&mut tx, session).await?;
    let mut events = repo::integrity_events(&mut tx, session).await?;
    tx.commit().await?;
    let mut prior = String::from("legacy-or-genesis");
    let mut expected_sequence = 0i64;
    for event in &mut events {
        expected_sequence += 1;
        let mut metadata = event["evidence"].clone();
        let signature = metadata
            .as_object_mut()
            .and_then(|v| v.remove("signature"));
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
            score += weights[e["evidence"]["kind"]
                .as_str()
                .unwrap_or("")]
            .as_u64()
            .unwrap_or(0);
        }
    }
    Ok(
        json!({"risk_score":score.min(100),"weights":weights,"events":events,"summary":format!("{} browser observations; {} weighted risk points. Review the evidence and context before drawing conclusions.",events.len(),score.min(100)),"notice":"Risk points are review priorities, not a probability or verdict of cheating. Confirmed activity means a reviewer confirmed that activity occurred, not cheating."}),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn question_clipboard_actions_have_no_integrity_weight() {
        let weights = super::weights();
        for kind in [
            "copy_from_question",
            "paste_from_question",
            "question_context_menu",
        ] {
            assert!(crate::proctoring::permitted_question_action(kind));
            assert_eq!(weights[kind], 0);
        }
        assert!(weights["paste"].as_u64().unwrap() > 0);
    }
}

pub(crate) async fn export_report(pool: &PgPool, owner: Uuid, session: Uuid) -> Result<Value> {
    let integrity_report = integrity(pool, owner, session).await?;
    let mut tx = interviewer_transaction(pool, owner).await?;
    exists(&mut tx, session).await?;
    let interview = repo::interview(&mut tx, session).await?;
    let questions = repo::questions(&mut tx, session).await?;
    let events = repo::events(&mut tx, session).await?;
    audit(
        &mut tx,
        owner,
        Some(owner),
        session,
        "report.exported",
        json!({"format":"json","version":1}),
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"version":1,"interview":interview,"questions":questions,"events":events,"integrity":integrity_report,"integrity_weights":weights(),"notice":"Scores and browser observations require human review. Identity signals must not influence scoring. Server signatures prove receipt integrity, not completeness or truth of browser telemetry."}),
    )
}
pub(crate) async fn analytics(pool: &PgPool, owner: Uuid, job: Uuid) -> Result<Value> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let found = repo::job_exists(&mut tx, job).await?;
    if !found {
        return Err(ApiError(StatusCode::NOT_FOUND, "Job not found"));
    }
    let groups = repo::analytics(&mut tx, job).await?;
    tx.commit().await?;
    Ok(
        json!({"groups":groups,"notice":"Descriptive statistics only. Unique questions and small samples cannot establish discrimination or fairness; validate with sufficient job-relevant outcomes."}),
    )
}
pub(crate) async fn metrics(pool: &PgPool, owner: Uuid) -> Result<Value> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let metrics = repo::metrics(&mut tx).await?;
    tx.commit().await?;
    Ok(metrics)
}

pub(crate) async fn validate_live(
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
    after: Option<Uuid>,
) -> Result<()> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    exists(&mut tx, session).await?;
    if let Some(after) = after {
        if !repo::cursor_exists(&mut tx, after, session).await? {
            return Err(ApiError(StatusCode::BAD_REQUEST, "Invalid event cursor"));
        }
    }
    tx.commit().await?;
    Ok(())
}
pub(crate) async fn live_page(
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
    after: Option<Uuid>,
) -> Result<Vec<Value>> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let rows = repo::live_events(&mut tx, session, after).await?;
    tx.commit().await?;
    Ok(rows)
}
