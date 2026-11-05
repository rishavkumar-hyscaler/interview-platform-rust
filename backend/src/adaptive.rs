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
    let provider = AiProvider::from_environment()
        .map_err(|_| ApiError(StatusCode::SERVICE_UNAVAILABLE, "AI is not configured"))?;
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
    let question:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',q.id,'position',q.position,'prompt',q.prompt,'rubric',q.rubric,'difficulty',q.difficulty,'answer',coalesce(a.content,''),'revision',coalesce(a.revision,0),'recordings',(SELECT count(*) FROM audit_events m WHERE m.entity_id=q.session_id AND m.action='answer.recorded' AND m.metadata->>'question_id'=q.id::text)) FROM interview_questions q LEFT JOIN interview_answers a ON a.question_id=q.id WHERE q.session_id=$1 AND q.id=$2").bind(session).bind(input.question_id).fetch_optional(&mut *tx).await?;
    let question = question.ok_or(ApiError(StatusCode::NOT_FOUND, "Question not found"))?;
    tx.commit().await?;
    let grade = crate::scoring::evaluate(&provider, &question)
        .await
        .map_err(|_| {
            ApiError(
                StatusCode::BAD_GATEWAY,
                "Unable to select adaptive question; use the question buttons to continue",
            )
        })?;
    let target = target_difficulty(
        question["difficulty"].as_str().unwrap_or("medium"),
        grade["score"].as_u64().unwrap_or(5),
    );
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
        "SELECT coalesce((SELECT revision FROM interview_answers WHERE question_id=$1),0)",
    )
    .bind(input.question_id)
    .fetch_one(&mut *tx)
    .await?;
    if question["revision"].as_i64() != Some(i64::from(revision)) {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Answer changed; request the next question again",
        ));
    }
    let position:Option<i32>=sqlx::query_scalar("SELECT position FROM interview_questions q WHERE session_id=$1 AND id<>$2 AND NOT EXISTS(SELECT 1 FROM audit_events a WHERE a.entity_id=$1 AND a.action='question.served' AND a.metadata->>'question_id'=q.id::text) ORDER BY abs((CASE difficulty WHEN 'easy' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END)-$3),position LIMIT 1").bind(session).bind(input.question_id).bind(target).fetch_optional(&mut *tx).await?;
    audit(&mut tx,owner,None,session,"adaptive.selected",json!({"question_id":input.question_id,"answer_revision":revision,"provisional_grade":grade,"target_difficulty":target,"next_position":position,"model":provider.model})).await?;
    tx.commit().await?;
    Ok(Json(json!({"position":position})))
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
}
