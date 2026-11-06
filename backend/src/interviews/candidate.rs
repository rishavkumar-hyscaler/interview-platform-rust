//! Candidate session projection and allowed state transitions.
use crate::{Result, access::candidate, audit::record as audit, error::conflict};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;
pub(crate) async fn candidate_view(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Value> {
    // Deliberately excludes rubrics, other candidates, email, and internal audit data.
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'role','interviewee','interviewee_id',s.interviewee_id,'job_title',j.title,'job_code',j.job_code,'state',CASE WHEN s.state IN ('invited','ready','in_progress') AND least(s.expires_at,coalesce(s.deadline_at,s.expires_at))<=now() THEN CASE WHEN s.state='in_progress' THEN 'submitted' ELSE 'expired' END ELSE s.state END,'duration_minutes',s.duration_minutes,'expires_at',s.expires_at,'deadline_at',s.deadline_at,'server_time',now(),'started_at',s.started_at,'submitted_at',s.submitted_at,'generation_status',coalesce((SELECT status FROM question_sets WHERE id=s.question_set_id),'not_started'),'questions_ready',EXISTS(SELECT 1 FROM question_sets p WHERE p.id=s.question_set_id AND p.status='completed' AND EXISTS(SELECT 1 FROM interview_questions q WHERE q.set_id=p.id)),'answer_count',(SELECT count(*) FROM session_question_details() q WHERE q.session_id=s.id AND (EXISTS(SELECT 1 FROM interview_answers a WHERE a.question_id=q.id AND a.session_id=q.session_id AND length(btrim(a.content))>0) OR EXISTS(SELECT 1 FROM audit_events m WHERE m.entity_id=q.session_id AND m.action='answer.recorded' AND m.metadata->>'question_id'=q.id::text))),'question_count',(SELECT count(*) FROM session_question_details() WHERE session_id=s.id)) FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.id=$1")
        .bind(id).fetch_one(&mut **tx).await?)
}
pub(super) async fn candidate_session(
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (owner, id, mut tx) = candidate(&pool, &headers).await?;
    crate::infrastructure::finalize_due_session(&mut tx, owner, id).await?;
    crate::generation::bank::assign(&mut tx, owner, id).await?;
    let result = candidate_view(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(result))
}
fn next_state(current: &str, action: &str) -> Result<&'static str> {
    match (current, action) {
        ("invited" | "ready", "ready") => Ok("ready"),
        ("ready" | "in_progress", "start") => Ok("in_progress"),
        ("in_progress" | "submitted", "submit") => Ok("submitted"),
        _ => Err(conflict("This session transition is not allowed")),
    }
}
pub(super) async fn candidate_transition(
    State(pool): State<PgPool>,
    Path(action): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (recruiter, id, mut tx) = candidate(&pool, &headers).await?;
    crate::infrastructure::finalize_due_session(&mut tx, recruiter, id).await?;
    let row=sqlx::query("SELECT state,EXISTS(SELECT 1 FROM question_sets p WHERE p.id=question_set_id AND p.status='completed') AS prepared,least(expires_at,coalesce(deadline_at,expires_at))>now() AS active FROM interview_sessions WHERE id=$1 FOR UPDATE")
        .bind(id).fetch_one(&mut *tx).await?;
    let current: String = row.get("state");
    let next = next_state(&current, &action)?;
    if next != current {
        if !row.get::<bool, _>("active") {
            return Err(conflict("Session has expired"));
        }
        if action == "start" {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM session_question_details() WHERE session_id=$1",
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            let prepared = row.get::<bool, _>("prepared");
            if count == 0 || !prepared {
                return Err(conflict("Questions are not prepared yet"));
            }
        }
        sqlx::query("UPDATE interview_sessions SET state=$2,started_at=CASE WHEN $2='in_progress' THEN now() ELSE started_at END,deadline_at=CASE WHEN $2='in_progress' THEN least(expires_at,now()+duration_minutes*interval '1 minute') ELSE deadline_at END,submitted_at=CASE WHEN $2='submitted' THEN now() ELSE submitted_at END WHERE id=$1")
            .bind(id).bind(next).execute(&mut *tx).await?;
        audit(
            &mut tx,
            recruiter,
            None,
            id,
            &format!("session.{next}"),
            json!({"actor":"candidate","previous_state":current}),
        )
        .await?;
    }
    if action == "ready" {
        crate::generation::bank::assign(&mut tx, recruiter, id).await?;
    }
    let result = candidate_view(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(result))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transitions_are_ordered_and_retries_are_safe() {
        assert_eq!(next_state("invited", "ready").unwrap(), "ready");
        assert_eq!(next_state("ready", "ready").unwrap(), "ready");
        assert_eq!(next_state("ready", "start").unwrap(), "in_progress");
        assert_eq!(next_state("in_progress", "start").unwrap(), "in_progress");
        assert_eq!(next_state("in_progress", "submit").unwrap(), "submitted");
        assert_eq!(next_state("submitted", "submit").unwrap(), "submitted");
        for (state, action) in [
            ("invited", "submit"),
            ("invited", "start"),
            ("submitted", "start"),
            ("expired", "ready"),
            ("completed", "submit"),
            ("ready", "completed"),
        ] {
            assert!(next_state(state, action).is_err());
        }
    }
}
