//! Candidate room and optimistic answer updates, with one transaction per action.
use crate::{
    ApiError, Result,
    audit::record as audit,
    infrastructure::finalize_due_session,
    models::answer::{Room, SavedAnswer},
    repositories::{answer_repository as repo, candidate_repository},
};
use axum::http::StatusCode;
use serde_json::json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) struct SaveAnswer {
    pub content: String,
    pub revision: i32,
}
pub(crate) async fn room(
    owner: Uuid,
    id: Uuid,
    mut tx: Transaction<'_, Postgres>,
    requested_position: Option<i32>,
) -> Result<Room> {
    finalize_due_session(&mut tx, owner, id).await?;
    let state = repo::lock_state(&mut tx, id).await?;
    let session = candidate_repository::view(&mut tx, id).await?;
    if matches!(
        state.as_str(),
        "submitted" | "scoring" | "completed" | "expired"
    ) {
        tx.commit().await?;
        return Ok(Room {
            session,
            question: None,
            question_started_at: None,
        });
    }
    if state != "in_progress" {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Start the interview before opening questions",
        ));
    }
    let previous = repo::previous(&mut tx, id).await?;
    let position = requested_position.unwrap_or_else(|| {
        previous
            .as_ref()
            .and_then(|p| p["position"].as_i64())
            .unwrap_or(1) as i32
    });
    if position < 1 {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Question position must be positive",
        ));
    }
    let question = repo::question(&mut tx, id, position).await?;
    let question = question.ok_or(ApiError(StatusCode::NOT_FOUND, "Question not found"))?;
    if previous
        .as_ref()
        .and_then(|p| p["position"].as_i64())
        != Some(i64::from(position))
    {
        audit(
            &mut tx,
            owner,
            None,
            id,
            "question.served",
            json!({"actor":"interviewee","question_id":question.id,"position":position}),
        )
        .await?;
    }
    let served_at = repo::served_at(&mut tx, id, position).await?;
    tx.commit().await?;
    Ok(Room {
        session,
        question: Some(question),
        question_started_at: Some(served_at),
    })
}
pub(crate) async fn save(
    owner: Uuid,
    id: Uuid,
    mut tx: Transaction<'_, Postgres>,
    question: Uuid,
    input: SaveAnswer,
) -> Result<SavedAnswer> {
    if input.content.len() > 48000
        || input.content.contains('\0')
        || !(0..i32::MAX).contains(&input.revision)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Answer must be at most 48000 bytes with a valid revision",
        ));
    }
    finalize_due_session(&mut tx, owner, id).await?;
    let state = repo::lock_state(&mut tx, id).await?;
    if state != "in_progress" {
        // Commit an automatic deadline finalization even when rejecting a late save.
        tx.commit().await?;
        return Err(ApiError(
            StatusCode::CONFLICT,
            "The interview has ended. Answers are now read-only",
        ));
    }
    let exists = repo::question_exists(&mut tx, question, id).await?;
    if !exists {
        return Err(ApiError(StatusCode::NOT_FOUND, "Question not found"));
    }
    let current = repo::current(&mut tx, id, question).await?;
    let (revision, changed) = if let Some(row) = current {
        let revision: i32 = row.revision;
        let content: String = row.content;
        if input.revision > revision || (input.revision != revision && content != input.content) {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "This answer changed in another tab. Resolve the saved version before continuing",
            ));
        }
        (
            if content == input.content {
                revision
            } else {
                revision.checked_add(1).ok_or(ApiError(
                    StatusCode::CONFLICT,
                    "Answer revision limit reached",
                ))?
            },
            content != input.content,
        )
    } else {
        if input.revision != 0 {
            return Err(ApiError(
                StatusCode::CONFLICT,
                "Saved answer revision is no longer available",
            ));
        }
        (1, true)
    };
    if changed {
        repo::save(&mut tx, owner, id, question, &input.content, revision).await?;
        audit(&mut tx,owner,None,id,"answer.saved",json!({"actor":"interviewee","question_id":question,"revision":revision,"bytes":input.content.len()})).await?;
    }
    let result = repo::saved(&mut tx, id, question).await?;
    tx.commit().await?;
    Ok(result)
}
