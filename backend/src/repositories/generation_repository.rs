//! Generation endpoint queries. Transactions belong to services.
use crate::{
    Result,
    models::generation::{JobPreparation, SessionGeneration},
};
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn session(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<SessionGeneration>> {
    Ok(sqlx::query_as(
        r#"
        SELECT coalesce(p.status,
                'not_started') AS status,
            CASE WHEN p.id IS NOT NULL THEN jsonb_build_object('id',
                p.id,
                'status',
                p.status,
                'attempts',
                p.attempts,
                'last_error',
                p.last_error) END AS job,
            j.blueprint
        FROM interview_sessions s JOIN jobs j ON j.id = s.job_id
        LEFT JOIN question_sets p ON p.id = s.question_set_id
        WHERE s.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn session_questions(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                id,
                'position',
                position,
                'prompt',
                prompt,
                'kind',
                kind,
                'difficulty',
                difficulty,
                'competency_id',
                competency_id,
                'rubric',
                rubric,
                'answer',
                (
                SELECT jsonb_build_object('content',
                        a.content,
                        'revision',
                        a.revision,
                        'updated_at',
                        a.updated_at)
                FROM interview_answers a
                WHERE a.question_id = session_question_details.id
                AND a.session_id = session_question_details.session_id))
        FROM session_question_details()
        WHERE session_id = $1
        ORDER BY position
        "#,
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn job(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    lock: bool,
) -> Result<Option<JobPreparation>> {
    let query = if lock {
        "SELECT blueprint,preparation_duration_minutes,question_set_count FROM jobs WHERE id=$1 FOR UPDATE"
    } else {
        "SELECT blueprint,preparation_duration_minutes,question_set_count FROM jobs WHERE id=$1"
    };
    Ok(sqlx::query_as(query)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?)
}
pub(crate) async fn sets(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                p.id,
                'status',
                p.status,
                'duration_minutes',
                p.duration_minutes,
                'attempts',
                p.attempts,
                'last_error',
                p.last_error,
                'assigned',
                false,
                'plan',
                (
                SELECT a.metadata -> 'plan'
                FROM audit_events a
                WHERE a.entity_id = p.id
                AND a.action = 'generation.planned'
                ORDER BY a.created_at,
                    a.id
                LIMIT 1),
                'usage_count',
                (
                SELECT count(*)
                FROM interview_sessions s
                WHERE s.question_set_id = p.id),
                'last_failure',
                (
                SELECT jsonb_build_object('phase',
                        a.metadata -> 'phase',
                        'summary',
                        a.metadata -> 'summary',
                        'question_position',
                        a.metadata -> 'question_position',
                        'findings',
                        a.metadata -> 'findings')
                FROM audit_events a
                WHERE a.entity_id = p.id
                AND a.action = 'generation.rejected'
                ORDER BY a.created_at DESC,
                    a.id DESC
                LIMIT 1),
                'question_count',
                (
                SELECT count(*)
                FROM interview_questions q
                WHERE q.set_id = p.id))
        FROM question_sets p
        WHERE p.job_id = $1
        AND p.id IN (
            SELECT id
            FROM question_sets
            WHERE job_id = $1
            ORDER BY created_at,
                id
            LIMIT (
                SELECT question_set_count
                FROM jobs
                WHERE id = $1))
        ORDER BY p.created_at DESC,
            p.id DESC
        LIMIT 60
        "#,
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn questions(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                q.id,
                'set_id',
                q.set_id,
                'position',
                q.position,
                'kind',
                q.kind,
                'difficulty',
                q.difficulty,
                'prompt',
                q.prompt,
                'rubric',
                q.rubric)
        FROM interview_questions q JOIN question_sets p ON p.id = q.set_id
        WHERE p.job_id = $1
        AND p.id IN (
            SELECT id
            FROM question_sets
            WHERE job_id = $1
            ORDER BY created_at,
                id
            LIMIT (
                SELECT question_set_count
                FROM jobs
                WHERE id = $1))
        ORDER BY p.created_at,
            p.id,
            q.position
        LIMIT 120
        "#,
    )
    .bind(id)
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn reset_failed(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE question_sets
        SET status = 'pending',
            attempts = 0,
            last_error = NULL,
            claim_token = NULL,
            lease_until = NULL,
            available_at = now()
        WHERE job_id = $1
        AND status = 'failed'
        AND id IN (
            SELECT id
            FROM question_sets
            WHERE job_id = $1
            ORDER BY created_at,
                id
            LIMIT (
                SELECT question_set_count
                FROM jobs
                WHERE id = $1))
        "#,
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
