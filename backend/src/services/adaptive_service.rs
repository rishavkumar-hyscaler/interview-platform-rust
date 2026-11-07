//! Adaptive grading is advisory and never blocks fallback navigation.
use crate::{
    ApiError, Result,
    audit::record as audit,
    generation::provider::AiProvider,
    infrastructure::{finalize_due_session, interviewer_transaction},
    repositories::adaptive_repository as repo,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use std::{future::Future, time::Duration};
use uuid::Uuid;
pub(crate) async fn next(
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
    mut tx: Transaction<'_, Postgres>,
    question_id: Uuid,
    session_hash: &str,
) -> Result<Option<i32>> {
    finalize_due_session(&mut tx, owner, session).await?;
    let state = repo::state(&mut tx, session).await?;
    if state != "in_progress" {
        tx.commit().await?;
        return Err(ApiError(StatusCode::CONFLICT, "Interview is not active"));
    }
    let question = repo::question(&mut tx, session, question_id).await?;
    let question = question.ok_or(ApiError(StatusCode::NOT_FOUND, "Question not found"))?;
    let cached = repo::cached(
        &mut tx,
        session,
        question_id,
        &question["revision"].to_string(),
    )
    .await?;
    tx.commit().await?;
    let (mut grade, mut selection) = if let Some(grade) = cached {
        (Some(grade), "cached_grade")
    } else if question["answer"]
        .as_str()
        .unwrap_or("")
        .trim()
        .is_empty()
    {
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
    if crate::repositories::auth_repository::candidate_for_token(pool, session_hash).await?
        != Some((owner, session))
    {
        return Err(crate::unauthorized());
    }
    let mut tx = interviewer_transaction(pool, owner).await?;
    finalize_due_session(&mut tx, owner, session).await?;
    let active = repo::active(&mut tx, session).await?;
    if !active {
        tx.commit().await?;
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Interview is no longer active",
        ));
    }
    let revision = repo::revision(&mut tx, question_id, session).await?;
    if question["revision"].as_i64() != Some(i64::from(revision)) {
        // Autosave can legitimately advance the revision during grading. Never use
        // that stale grade or make the candidate retry just to navigate.
        grade = None;
        selection = "answer_changed";
    }
    let target = grade
        .as_ref()
        .and_then(|g| g["score"].as_u64())
        .map(|score| {
            target_difficulty(
                question["difficulty"]
                    .as_str()
                    .unwrap_or("medium"),
                score,
            )
        });
    let position: Option<i32> = if let Some(target) = target {
        repo::target_position(&mut tx, session, question_id, target).await?
    } else {
        None
    };
    let position = match position {
        Some(position) => Some(position),
        None => {
            repo::next_position(
                &mut tx,
                session,
                question["position"]
                    .as_i64()
                    .unwrap_or(0) as i32,
            )
            .await?
        }
    };
    audit(&mut tx,owner,None,session,"adaptive.selected",json!({"question_id":question_id,"answer_revision":revision,"provisional_grade":grade,"target_difficulty":target,"next_position":position,"selection":selection})).await?;
    tx.commit().await?;
    Ok(position)
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
