//! Interviewee-only question delivery and optimistic answer saves.
use crate::{
    ApiError, Result,
    infrastructure::finalize_due_session,
    interviews::{audit, candidate, candidate_view},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub fn routes() -> Router<PgPool> {
    Router::new()
        .route("/api/v1/candidate/room", get(room))
        .route("/api/v1/candidate/answers/{question}", post(save))
}
#[derive(Deserialize)]
struct Cursor {
    position: Option<i32>,
}
async fn room(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(cursor): Query<Cursor>,
) -> Result<Json<Value>> {
    let (owner, id, mut tx) = candidate(&pool, &headers).await?;
    finalize_due_session(&mut tx, owner, id).await?;
    let state: String =
        sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let session = candidate_view(&mut tx, id).await?;
    if matches!(
        state.as_str(),
        "submitted" | "scoring" | "completed" | "expired"
    ) {
        tx.commit().await?;
        return Ok(Json(json!({"session":session,"question":null})));
    }
    if state != "in_progress" {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Start the interview before opening questions",
        ));
    }
    let previous:Option<Value>=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE entity_id=$1 AND action='question.served' ORDER BY created_at DESC,id DESC LIMIT 1")
        .bind(id).fetch_optional(&mut *tx).await?;
    let position = cursor.position.unwrap_or_else(|| {
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
    let question:Option<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',q.id,'position',q.position,'prompt',q.prompt,'kind',q.kind,'difficulty',q.difficulty,'answer',coalesce(a.content,''),'revision',coalesce(a.revision,0),'saved_at',a.updated_at) FROM interview_questions q LEFT JOIN interview_answers a ON a.question_id=q.id AND a.session_id=q.session_id WHERE q.session_id=$1 AND q.position=$2")
        .bind(id).bind(position).fetch_optional(&mut *tx).await?;
    let question = question.ok_or(ApiError(StatusCode::NOT_FOUND, "Question not found"))?;
    if previous.as_ref().and_then(|p| p["position"].as_i64()) != Some(i64::from(position)) {
        audit(
            &mut tx,
            owner,
            None,
            id,
            "question.served",
            json!({"actor":"interviewee","question_id":question["id"],"position":position}),
        )
        .await?;
    }
    let served_at:Value=sqlx::query_scalar("SELECT to_jsonb(created_at) FROM audit_events WHERE entity_id=$1 AND action='question.served' AND metadata->>'position'=$2 ORDER BY created_at DESC,id DESC LIMIT 1")
        .bind(id).bind(position.to_string()).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"session":session,"question":question,"question_started_at":served_at}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Answer {
    content: String,
    revision: i32,
}
async fn save(
    State(pool): State<PgPool>,
    Path(question): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<Answer>,
) -> Result<Json<Value>> {
    if input.content.len() > 48000
        || input.content.contains('\0')
        || !(0..i32::MAX).contains(&input.revision)
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Answer must be at most 48000 bytes with a valid revision",
        ));
    }
    let (owner, id, mut tx) = candidate(&pool, &headers).await?;
    finalize_due_session(&mut tx, owner, id).await?;
    let state: String =
        sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    if state != "in_progress" {
        // Commit an automatic deadline finalization even when rejecting a late save.
        tx.commit().await?;
        return Err(ApiError(
            StatusCode::CONFLICT,
            "The interview has ended. Answers are now read-only",
        ));
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM interview_questions WHERE id=$1 AND session_id=$2)",
    )
    .bind(question)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(ApiError(StatusCode::NOT_FOUND, "Question not found"));
    }
    let current=sqlx::query("SELECT content,revision FROM interview_answers WHERE session_id=$1 AND question_id=$2 FOR UPDATE").bind(id).bind(question).fetch_optional(&mut *tx).await?;
    let (revision, changed) = if let Some(row) = current {
        let revision: i32 = row.get("revision");
        let content: String = row.get("content");
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
        sqlx::query("INSERT INTO interview_answers(id,interviewer_id,session_id,question_id,content,revision) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(session_id,question_id) DO UPDATE SET content=excluded.content,revision=excluded.revision,updated_at=now()")
            .bind(Uuid::new_v4()).bind(owner).bind(id).bind(question).bind(&input.content).bind(revision).execute(&mut *tx).await?;
        audit(&mut tx,owner,None,id,"answer.saved",json!({"actor":"interviewee","question_id":question,"revision":revision,"bytes":input.content.len()})).await?;
    }
    let result:Value=sqlx::query_scalar("SELECT jsonb_build_object('question_id',question_id,'revision',revision,'saved_at',updated_at,'answer_count',(SELECT count(*) FROM interview_questions q WHERE q.session_id=$1 AND (EXISTS(SELECT 1 FROM interview_answers a WHERE a.question_id=q.id AND length(btrim(a.content))>0) OR EXISTS(SELECT 1 FROM audit_events m WHERE m.entity_id=q.session_id AND m.action='answer.recorded' AND m.metadata->>'question_id'=q.id::text)))) FROM interview_answers WHERE session_id=$1 AND question_id=$2")
        .bind(id).bind(question).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(result))
}
