//! Interviewer session query projections; all fields selected explicitly.
use crate::{
    Result,
    models::interview::{SessionDetail, SessionListItem, Summary},
};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn list(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    cursor: Option<Uuid>,
    limit: i64,
) -> Result<Vec<SessionListItem>> {
    Ok(sqlx::query_as(
        r#"
        SELECT s.id,
            s.interviewer_id,
            s.job_id,
            s.question_set_id,
            s.candidate_name,
            s.candidate_email,
            s.state,
            s.duration_minutes,
            to_jsonb(s.expires_at) #>> '{}' AS expires_at,
            to_jsonb(s.started_at) #>> '{}' AS started_at,
            to_jsonb(s.deadline_at) #>> '{}' AS deadline_at,
            to_jsonb(s.submitted_at) #>> '{}' AS submitted_at,
            to_jsonb(s.created_at) #>> '{}' AS created_at,
            s.interviewee_id,
            j.title AS job_title,
            j.job_code
        FROM interview_sessions s JOIN jobs j ON j.id = s.job_id
        WHERE s.interviewer_id = $1
        AND ($2::uuid IS NULL
            OR (s.created_at,
                    s.id) < (
                SELECT created_at,
                    id
                FROM interview_sessions
                WHERE id = $2))
        ORDER BY s.created_at DESC,
            s.id DESC
        LIMIT $3
        "#,
    )
    .bind(owner)
    .bind(cursor)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn detail(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    id: Uuid,
) -> Result<Option<SessionDetail>> {
    Ok(sqlx::query_as(
        r#"
        SELECT s.id,
            s.interviewer_id,
            s.job_id,
            s.question_set_id,
            s.candidate_name,
            s.candidate_email,
            s.state,
            s.duration_minutes,
            to_jsonb(s.expires_at) #>> '{}' AS expires_at,
            to_jsonb(s.started_at) #>> '{}' AS started_at,
            to_jsonb(s.deadline_at) #>> '{}' AS deadline_at,
            to_jsonb(s.submitted_at) #>> '{}' AS submitted_at,
            to_jsonb(s.created_at) #>> '{}' AS created_at,
            s.interviewee_id,
            j.title AS job_title,
            j.job_code,
            j.description,
            (
            SELECT count(*)
            FROM session_question_details() q
            WHERE q.session_id = s.id) AS question_count,
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
                    AND m.metadata ->> 'question_id' = q.id::text))) AS answer_count,
            coalesce((
                SELECT sealed_token IS NOT NULL
                FROM auth_sessions c
                WHERE c.interview_session_id = s.id),
                false) AS invitation_recoverable,
            s.state IN ('invited',
                'ready',
                'in_progress')
        AND least(s.expires_at,
                coalesce(s.deadline_at,
                    s.expires_at)) > now() AS invitation_active
        FROM interview_sessions s JOIN jobs j ON j.id = s.job_id
        WHERE s.id = $1
        AND s.interviewer_id = $2
        "#,
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn summary(tx: &mut Transaction<'_, Postgres>) -> Result<Summary> {
    Ok(sqlx::query_as(
        r#"
        SELECT (
            SELECT count(*)
            FROM jobs) AS jobs,
            count(*) AS sessions,
            count(*) FILTER (
            WHERE state IN ('ready',
                    'in_progress')
            AND least(expires_at,
                    coalesce(deadline_at,
                        expires_at)) > now()) AS active,
            count(*) FILTER (
            WHERE state IN ('submitted',
                    'scoring',
                    'completed')) AS reviewable
        FROM interview_sessions
        "#,
    )
    .fetch_one(&mut **tx)
    .await?)
}
