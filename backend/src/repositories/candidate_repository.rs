//! Candidate session projections and transitions, scoped by caller transaction.
use crate::{
    Result,
    models::candidate::{SessionSnapshot, TransitionState},
};
use sqlx::{Postgres, Transaction, types::Json};
use uuid::Uuid;
pub(crate) async fn view(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<SessionSnapshot> {
    let value:Json<SessionSnapshot>=sqlx::query_scalar(r#"
        SELECT jsonb_build_object('id',
                s.id,
                'role',
                'interviewee',
                'interviewee_id',
                s.interviewee_id,
                'job_title',
                j.title,
                'job_code',
                j.job_code,
                'state',
                CASE WHEN s.state IN ('invited',
                    'ready',
                    'in_progress')
            AND least(s.expires_at,
                    coalesce(s.deadline_at,
                        s.expires_at)) <= now() THEN CASE WHEN s.state = 'in_progress' THEN 'submitted' ELSE 'expired' END ELSE s.state END,
                'duration_minutes',
                s.duration_minutes,
                'expires_at',
                s.expires_at,
                'deadline_at',
                s.deadline_at,
                'server_time',
                now(),
                'started_at',
                s.started_at,
                'submitted_at',
                s.submitted_at,
                'generation_status',
                coalesce((
                    SELECT status
                    FROM question_sets
                    WHERE id = s.question_set_id),
                    'not_started'),
                'questions_ready',
                EXISTS (
                SELECT 1
                FROM question_sets p
                WHERE p.id = s.question_set_id
                AND p.status = 'completed'
                AND EXISTS (
                    SELECT 1
                    FROM interview_questions q
                    WHERE q.set_id = p.id)),
                'answer_count',
                (
                SELECT count(*)
                FROM session_question_details() q
                WHERE q.session_id = s.id
                AND (EXISTS (
                        SELECT 1
                        FROM interview_answers a
                        WHERE a.question_id = q.id
                        AND a.session_id = q.session_id
                        AND length(btrim(a.content)) > 0)
                    OR EXISTS (
                        SELECT 1
                        FROM audit_events m
                        WHERE m.entity_id = q.session_id
                        AND m.action = 'answer.recorded'
                        AND m.metadata ->> 'question_id' = q.id::text))),
                'question_count',
                (
                SELECT count(*)
                FROM session_question_details()
                WHERE session_id = s.id))
        FROM interview_sessions s JOIN jobs j ON j.id = s.job_id
        WHERE s.id = $1
        "#).bind(id).fetch_one(&mut **tx).await?;
    Ok(value.0)
}
pub(crate) async fn lock_state(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<TransitionState> {
    Ok(sqlx::query_as(
        r#"
        SELECT state,
            EXISTS (
            SELECT 1
            FROM question_sets p
            WHERE p.id = question_set_id
            AND p.status = 'completed') AS prepared,
            least(expires_at,
                coalesce(deadline_at,
                    expires_at)) > now() AS active
        FROM interview_sessions
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn question_count(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT count(*)
        FROM session_question_details()
        WHERE session_id = $1
        "#,
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn transition(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    next: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE interview_sessions
        SET state = $2,
            started_at = CASE WHEN $2 = 'in_progress' THEN now() ELSE started_at END,
            deadline_at = CASE WHEN $2 = 'in_progress' THEN least(expires_at,
                now() + duration_minutes * interval '1 minute') ELSE deadline_at END,
            submitted_at = CASE WHEN $2 = 'submitted' THEN now() ELSE submitted_at END
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(next)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
