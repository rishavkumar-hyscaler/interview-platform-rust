//! Adaptive selection queries. Grading and fallback decisions live in the service.
use crate::Result;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn state(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1")
            .bind(session)
            .fetch_one(&mut **tx)
            .await?,
    )
}
pub(crate) async fn question(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    question: Uuid,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('id',q.id,'position',q.position,'prompt',q.prompt,'rubric',q.rubric,'difficulty',q.difficulty,'answer',coalesce(a.content,''),'revision',coalesce(a.revision,0),'recordings',(SELECT count(*) FROM audit_events m WHERE m.entity_id=q.session_id AND m.action='answer.recorded' AND m.metadata->>'question_id'=q.id::text)) FROM session_question_details() q LEFT JOIN interview_answers a ON a.question_id=q.id AND a.session_id=q.session_id WHERE q.session_id=$1 AND q.id=$2").bind(session).bind(question).fetch_optional(&mut **tx).await?)
}
pub(crate) async fn cached(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    question: Uuid,
    revision: &str,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar("SELECT metadata->'provisional_grade' FROM audit_events WHERE entity_id=$1 AND action='adaptive.selected' AND metadata->>'question_id'=$2 AND metadata->>'answer_revision'=$3 AND metadata->'provisional_grade'->'score' IS NOT NULL ORDER BY created_at DESC,id DESC LIMIT 1").bind(session).bind(question.to_string()).bind(revision).fetch_optional(&mut **tx).await?)
}
pub(crate) async fn active(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT state='in_progress' FROM interview_sessions WHERE id=$1")
            .bind(session)
            .fetch_one(&mut **tx)
            .await?,
    )
}
pub(crate) async fn revision(
    tx: &mut Transaction<'_, Postgres>,
    question: Uuid,
    session: Uuid,
) -> Result<i32> {
    Ok(sqlx::query_scalar("SELECT coalesce((SELECT revision FROM interview_answers WHERE question_id=$1 AND session_id=$2),0)").bind(question).bind(session).fetch_one(&mut **tx).await?)
}
pub(crate) async fn target_position(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    question: Uuid,
    target: i32,
) -> Result<Option<i32>> {
    Ok(sqlx::query_scalar("SELECT position FROM session_question_details() q WHERE session_id=$1 AND id<>$2 AND NOT EXISTS(SELECT 1 FROM audit_events a WHERE a.entity_id=$1 AND a.action='question.served' AND a.metadata->>'question_id'=q.id::text) ORDER BY abs((CASE difficulty WHEN 'easy' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END)-$3),position LIMIT 1").bind(session).bind(question).bind(target).fetch_optional(&mut **tx).await?)
}
pub(crate) async fn next_position(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    position: i32,
) -> Result<Option<i32>> {
    Ok(sqlx::query_scalar("SELECT position FROM session_question_details() WHERE session_id=$1 AND position>$2 ORDER BY position LIMIT 1").bind(session).bind(position).fetch_optional(&mut **tx).await?)
}
