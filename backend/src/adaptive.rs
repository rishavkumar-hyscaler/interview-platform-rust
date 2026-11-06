use crate::{
    ApiError, Result, access::candidate, audit::record as audit, generation::provider::AiProvider,
    infrastructure::finalize_due_session,
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{future::Future, time::Duration};
use uuid::Uuid;
pub fn routes() -> Router<PgPool> {
    Router::new().route("/api/v1/candidate/recommendation", post(next))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Current {
    question_id: Uuid,
}
async fn next(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(input): Json<Current>,
) -> Result<Json<Value>> {
    let (owner, session, mut tx) = candidate(&pool, &headers).await?;
    finalize_due_session(&mut tx, owner, session).await?;
    let state: String = sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1")
        .bind(session)
        .fetch_one(&mut *tx)
        .await?;
    if state != "in_progress" {
        tx.commit().await?;
        return Err(ApiError(StatusCode::CONFLICT, "Interview is not active"));
    }
    let question:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',q.id,'position',q.position,'prompt',q.prompt,'rubric',q.rubric,'difficulty',q.difficulty,'answer',coalesce(a.content,''),'revision',coalesce(a.revision,0),'recordings',(SELECT count(*) FROM audit_events m WHERE m.entity_id=q.session_id AND m.action='answer.recorded' AND m.metadata->>'question_id'=q.id::text)) FROM session_question_details() q LEFT JOIN interview_answers a ON a.question_id=q.id AND a.session_id=q.session_id WHERE q.session_id=$1 AND q.id=$2").bind(session).bind(input.question_id).fetch_optional(&mut *tx).await?;
    let question = question.ok_or(ApiError(StatusCode::NOT_FOUND, "Question not found"))?;
    let cached:Option<Value>=sqlx::query_scalar("SELECT metadata->'provisional_grade' FROM audit_events WHERE entity_id=$1 AND action='adaptive.selected' AND metadata->>'question_id'=$2 AND metadata->>'answer_revision'=$3 AND metadata->'provisional_grade'->'score' IS NOT NULL ORDER BY created_at DESC,id DESC LIMIT 1")
        .bind(session).bind(input.question_id.to_string()).bind(question["revision"].to_string()).fetch_optional(&mut *tx).await?;
    tx.commit().await?;
    let (mut grade, mut selection) = if let Some(grade) = cached {
        (Some(grade), "cached_grade")
    } else if question["answer"].as_str().unwrap_or("").trim().is_empty() {
        // An unanswered/media-only question cannot justify an adaptive difficulty change.
        (None, "no_written_answer")
    } else {
        match AiProvider::from_environment() {
            Ok(provider) => {
                bounded_grade(
                    crate::scoring::evaluate(&provider, &question),
                    Duration::from_secs(6),
                )
                .await
            }
            Err(_) => (None, "ai_unavailable"),
        }
    };
    let (_, _, mut tx) = candidate(&pool, &headers).await?;
    finalize_due_session(&mut tx, owner, session).await?;
    let active: bool =
        sqlx::query_scalar("SELECT state='in_progress' FROM interview_sessions WHERE id=$1")
            .bind(session)
            .fetch_one(&mut *tx)
            .await?;
    if !active {
        tx.commit().await?;
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Interview is no longer active",
        ));
    }
    let revision: i32 = sqlx::query_scalar(
        "SELECT coalesce((SELECT revision FROM interview_answers WHERE question_id=$1 AND session_id=$2),0)",
    )
    .bind(input.question_id)
    .bind(session)
    .fetch_one(&mut *tx)
    .await?;
    if question["revision"].as_i64() != Some(i64::from(revision)) {
        // Autosave can legitimately advance the revision during grading. Never use
        // that stale grade or make the candidate retry just to navigate.
        grade = None;
        selection = "answer_changed";
    }
    let target = grade
        .as_ref()
        .and_then(|g| g["score"].as_u64())
        .map(|score| target_difficulty(question["difficulty"].as_str().unwrap_or("medium"), score));
    let position: Option<i32> = if let Some(target) = target {
        sqlx::query_scalar("SELECT position FROM session_question_details() q WHERE session_id=$1 AND id<>$2 AND NOT EXISTS(SELECT 1 FROM audit_events a WHERE a.entity_id=$1 AND a.action='question.served' AND a.metadata->>'question_id'=q.id::text) ORDER BY abs((CASE difficulty WHEN 'easy' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END)-$3),position LIMIT 1")
            .bind(session).bind(input.question_id).bind(target).fetch_optional(&mut *tx).await?
    } else {
        None
    };
    let position = match position {
        Some(position) => Some(position),
        None => sqlx::query_scalar("SELECT position FROM session_question_details() WHERE session_id=$1 AND position>$2 ORDER BY position LIMIT 1")
            .bind(session).bind(question["position"].as_i64().unwrap_or(0) as i32).fetch_optional(&mut *tx).await?,
    };
    audit(&mut tx,owner,None,session,"adaptive.selected",json!({"question_id":input.question_id,"answer_revision":revision,"provisional_grade":grade,"target_difficulty":target,"next_position":position,"selection":selection})).await?;
    tx.commit().await?;
    Ok(Json(json!({"position":position})))
}
// Selection is advisory: generation outages, bad citations and slow models must
// not prevent a saved answer from advancing to an already-prepared question.
async fn bounded_grade(
    work: impl Future<Output = anyhow::Result<Value>>,
    timeout: Duration,
) -> (Option<Value>, &'static str) {
    match tokio::time::timeout(timeout, work).await {
        Ok(Ok(grade)) => (Some(grade), "live_grade"),
        Ok(Err(_)) => {
            tracing::warn!(
                operation = "adaptive.select",
                "Grading unavailable; using prepared question order"
            );
            (None, "grade_failed")
        }
        Err(_) => {
            tracing::warn!(
                operation = "adaptive.select",
                "Grading time limit reached; using prepared question order"
            );
            (None, "grade_timeout")
        }
    }
}
fn target_difficulty(current: &str, score: u64) -> i32 {
    let level = match current {
        "easy" => 0,
        "hard" => 2,
        _ => 1,
    };
    if score >= 8 {
        (level + 1).min(2)
    } else if score <= 4 {
        (level - 1).max(0)
    } else {
        level
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strong_harder_weak_easier() {
        assert_eq!(target_difficulty("medium", 9), 2);
        assert_eq!(target_difficulty("medium", 3), 0);
        assert_eq!(target_difficulty("medium", 6), 1);
    }
    #[tokio::test]
    async fn grading_errors_and_timeouts_are_advisory() {
        let (grade, reason) = bounded_grade(
            std::future::ready(Err(anyhow::anyhow!("Invalid quote"))),
            Duration::from_secs(1),
        )
        .await;
        assert!(grade.is_none());
        assert_eq!(reason, "grade_failed");
        let (grade, reason) = bounded_grade(std::future::pending(), Duration::from_millis(1)).await;
        assert!(grade.is_none());
        assert_eq!(reason, "grade_timeout");
        let (grade, reason) = bounded_grade(
            std::future::ready(Ok(json!({"score":8}))),
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(grade.unwrap()["score"], 8);
        assert_eq!(reason, "live_grade");
    }
}
