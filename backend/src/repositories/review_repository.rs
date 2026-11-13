//! Session review persistence. Every call uses the caller's scoped transaction.
use crate::Result;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn session_exists(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM interview_sessions WHERE id=$1)")
            .bind(session)
            .fetch_one(&mut **tx)
            .await?,
    )
}
pub(crate) async fn observation_with_media_exists(
    tx: &mut Transaction<'_, Postgres>,
    event: Uuid,
    session: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM audit_events observation
            WHERE observation.id = $1
            AND observation.entity_id = $2
            AND observation.action = 'proctor.observed'
            AND EXISTS (
                SELECT 1
                FROM audit_events attachment
                WHERE attachment.entity_id = observation.entity_id
                AND attachment.action = 'proctor.evidence'
                AND attachment.metadata ->> 'event_id' = observation.id::text
            )
        )
        "#,
    )
    .bind(event)
    .bind(session)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn integrity_events(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                a.id,
                'received_at',
                a.created_at,
                'evidence',
                a.metadata,
                'attachments',
                (
                SELECT coalesce(jsonb_agg(e.metadata || jsonb_build_object('saved_at',
                            e.created_at)
                        ORDER BY e.created_at,
                            e.id),
                        '[]'::jsonb)
                FROM audit_events e
                WHERE e.entity_id = a.entity_id
                AND e.action = 'proctor.evidence'
                AND e.metadata ->> 'event_id' = a.id::text),
                'annotation',
                (
                SELECT jsonb_build_object('reviewer',
                        b.actor_id,
                        'at',
                        b.created_at,
                        'decision',
                        b.metadata ->> 'decision',
                        'note',
                        b.metadata ->> 'note')
                FROM audit_events b
                WHERE b.entity_id = a.entity_id
                AND b.action = 'integrity.annotated'
                AND b.metadata ->> 'event_id' = a.id::text
                ORDER BY b.created_at DESC,
                    b.id DESC
                LIMIT 1))
        FROM audit_events a
        WHERE entity_id = $1
        AND action = 'proctor.observed'
        ORDER BY (a.metadata ->> 'sequence')::bigint NULLS FIRST,
            a.created_at,
            a.id
        LIMIT 10000
        "#,
    )
    .bind(session)
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn cursor_exists(
    tx: &mut Transaction<'_, Postgres>,
    after: Uuid,
    session: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM audit_events
            WHERE id = $1
            AND entity_id = $2)
        "#,
    )
    .bind(after)
    .bind(session)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn live_events(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    after: Option<Uuid>,
) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                id,
                'action',
                action,
                'created_at',
                created_at,
                'metadata',
                metadata)
        FROM audit_events
        WHERE entity_id = $1
        AND action NOT IN ('reviewer.indexed',
                'reviewer.message')
        AND ($2::uuid IS NULL
            OR (created_at,
                    id) > (
                SELECT created_at,
                    id
                FROM audit_events
                WHERE id = $2
                AND entity_id = $1))
        ORDER BY created_at,
            id
        LIMIT 100
        "#,
    )
    .bind(session)
    .bind(after)
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn interview(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<Value> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                s.id,
                'job_title',
                j.title,
                'job_code',
                j.job_code,
                'job_description',
                j.description,
                'state',
                s.state,
                'started_at',
                s.started_at,
                'submitted_at',
                s.submitted_at,
                'candidate_name',
                s.candidate_name,
                'candidate_email',
                s.candidate_email)
        FROM interview_sessions s JOIN jobs j ON j.id = s.job_id
        WHERE s.id = $1
        "#,
    )
    .bind(session)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn questions(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                q.id,
                'position',
                q.position,
                'prompt',
                q.prompt,
                'rubric',
                q.rubric,
                'answer',
                a.content,
                'saved_at',
                a.updated_at)
        FROM session_question_details() q
        LEFT JOIN interview_answers a ON a.question_id = q.id
        AND a.session_id = q.session_id
        WHERE q.session_id = $1
        ORDER BY q.position
        "#,
    )
    .bind(session)
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn events(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                id,
                'at',
                created_at,
                'action',
                action,
                'actor_id',
                actor_id,
                'evidence',
                metadata)
        FROM audit_events
        WHERE entity_id = $1
        AND action NOT IN ('reviewer.indexed',
                'invitation.link_retrieved')
        ORDER BY created_at,
            id
        "#,
    )
    .bind(session)
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn job_exists(tx: &mut Transaction<'_, Postgres>, job: Uuid) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM jobs WHERE id=$1)")
            .bind(job)
            .fetch_one(&mut **tx)
            .await?,
    )
}
pub(crate) async fn analytics_sets(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object(
            'set_number', row_number() OVER (ORDER BY created_at, id),
            'status', status
        )
        FROM question_sets
        WHERE job_id = $1
        ORDER BY created_at, id
        "#,
    )
    .bind(job)
    .fetch_all(&mut **tx)
    .await?)
}

pub(crate) async fn analytics(tx: &mut Transaction<'_, Postgres>, job: Uuid) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(r#"
        WITH numbered_sets AS (
            SELECT id, status, row_number() OVER (ORDER BY created_at, id) AS set_number
            FROM question_sets
            WHERE job_id = $1
        ),
        completed AS (
            SELECT DISTINCT ON (a.entity_id) a.entity_id, a.metadata
            FROM audit_events a
            JOIN interview_sessions s ON s.id = a.entity_id
            WHERE a.action = 'assessment.completed' AND s.job_id = $1
            ORDER BY a.entity_id, a.created_at DESC, a.id DESC
        ),
        scores AS (
            SELECT
                entity_id,
                question ->> 'question_id' AS question_id,
                (question ->> 'score')::numeric AS points,
                (question ->> 'max_score')::numeric AS maximum
            FROM completed
            CROSS JOIN LATERAL jsonb_array_elements(metadata -> 'questions') question
        ),
        comparisons AS (
            SELECT
                *,
                sum(points) OVER (PARTITION BY entity_id) - points AS rest_points,
                sum(maximum) OVER (PARTITION BY entity_id) - maximum AS rest_maximum
            FROM scores
            WHERE points IS NOT NULL AND maximum > 0
        ),
        visits AS (
            SELECT
                s.id AS session_id,
                a.metadata ->> 'question_id' AS question_id,
                a.created_at AS opened_at,
                lead(a.created_at) OVER (
                    PARTITION BY s.id ORDER BY a.created_at, a.id
                ) AS next_opened_at,
                least(s.submitted_at, s.deadline_at) AS ended_at
            FROM interview_sessions s
            JOIN audit_events a ON a.entity_id = s.id
                AND a.interviewer_id = s.interviewer_id
            WHERE s.job_id = $1 AND s.submitted_at IS NOT NULL
                AND a.action = 'question.served'
        ),
        time_per_session AS (
            SELECT question_id, session_id,
                sum(extract(epoch FROM (least(next_opened_at, ended_at) - opened_at))) AS seconds
            FROM visits
            WHERE opened_at < ended_at AND question_id IS NOT NULL
            GROUP BY question_id, session_id
        ),
        question_times AS (
            SELECT question_id, avg(seconds) AS average_seconds, count(*) AS sample_size
            FROM time_per_session
            GROUP BY question_id
        )
        SELECT jsonb_build_object(
            'question_id', q.id,
            'set_number', qs.set_number,
            'position', q.position,
            'prompt', q.prompt,
            'kind', q.kind,
            'difficulty', q.difficulty,
            'competency_id', q.competency_id,
            'sample_size', count(c.question_id),
            'attempt_count', coalesce(qt.sample_size, 0),
            'paired_question_points', avg(10 * c.points / c.maximum) FILTER (WHERE c.rest_maximum > 0),
            'paired_rest_points', avg(10 * c.rest_points / nullif(c.rest_maximum, 0)),
            'average_seconds', qt.average_seconds,
            'timing_count', coalesce(qt.sample_size, 0),
            'comparison_count', count(*) FILTER (WHERE c.rest_maximum > 0),
            'mean_points', avg(10 * c.points / c.maximum),
            'item_total_correlation', corr(
                (c.points / c.maximum)::double precision,
                (c.rest_points / nullif(c.rest_maximum, 0))::double precision
            )
        )
        FROM interview_questions q
        JOIN numbered_sets qs ON qs.id = q.set_id AND qs.status = 'completed'
        LEFT JOIN comparisons c ON c.question_id = q.id::text
        LEFT JOIN question_times qt ON qt.question_id = q.id::text
        WHERE q.job_id = $1
        GROUP BY q.id, qt.average_seconds, qt.sample_size, qs.set_number
        ORDER BY qs.set_number, q.position, q.id
        "#).bind(job).fetch_all(&mut **tx).await?)
}
pub(crate) async fn metrics(tx: &mut Transaction<'_, Postgres>) -> Result<Value> {
    Ok(sqlx::query_scalar(r#"
        WITH timings AS (
            SELECT a.entity_id,
                a.action,
                extract(epoch
                FROM (a.created_at - (
                        SELECT min(b.created_at)
                        FROM audit_events b
                        WHERE b.entity_id = a.entity_id
                        AND b.action = CASE WHEN a.action = 'assessment.completed' THEN 'assessment.started' ELSE 'questions.queued' END))) * 1000 AS ms
            FROM audit_events a
            WHERE a.action IN ('assessment.completed',
                    'questions.prepared'))
        SELECT jsonb_build_object('assessment_p50_ms',
                (
                SELECT percentile_cont(0.5) WITHIN GROUP (
                    ORDER BY ms)
                FROM timings
                WHERE action = 'assessment.completed'),
                'assessment_p99_ms',
                (
                SELECT percentile_cont(0.99) WITHIN GROUP (
                    ORDER BY ms)
                FROM timings
                WHERE action = 'assessment.completed'),
                'generation_p50_ms',
                (
                SELECT percentile_cont(0.5) WITHIN GROUP (
                    ORDER BY ms)
                FROM timings
                WHERE action = 'questions.prepared'),
                'generation_p99_ms',
                (
                SELECT percentile_cont(0.99) WITHIN GROUP (
                    ORDER BY ms)
                FROM timings
                WHERE action = 'questions.prepared'),
                'sessions',
                (
                SELECT count(*)
                FROM interview_sessions),
                'observations',
                (
                SELECT count(*)
                FROM audit_events
                WHERE action = 'proctor.observed'),
                'scoring_failures',
                (
                SELECT count(*)
                FROM audit_events
                WHERE action = 'assessment.failed'),
                'observations_per_session',
                (
                SELECT count(*)::numeric / nullif((
                        SELECT count(*)
                        FROM interview_sessions),
                        0)
                FROM audit_events
                WHERE action = 'proctor.observed'))
        "#).fetch_one(&mut **tx).await?)
}
pub(crate) async fn memory(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    owner: Uuid,
    conversation: Uuid,
) -> Result<Vec<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT metadata - 'vectors'
        FROM audit_events
        WHERE entity_id = $1
        AND actor_id = $2
        AND action = 'reviewer.message'
        AND metadata ->> 'conversation' = $3
        ORDER BY created_at DESC,
            id DESC
        LIMIT 6
        "#,
    )
    .bind(session)
    .bind(owner)
    .bind(conversation.to_string())
    .fetch_all(&mut **tx)
    .await?)
}
pub(crate) async fn integrity_summary(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    fingerprint: &str,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT metadata
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'integrity.summarized'
        AND metadata ->> 'fingerprint' = $2
        ORDER BY created_at DESC,
            id DESC
        LIMIT 1
        "#,
    )
    .bind(session)
    .bind(fingerprint)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn assessment(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Option<(Uuid, Value)>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id,
            metadata
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'assessment.completed'
        ORDER BY created_at DESC,
            id DESC
        LIMIT 1
        "#,
    )
    .bind(session)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn counts(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as(
        r#"
        SELECT coalesce(metadata ->> 'kind',
                'unknown'),
            count(*)
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'proctor.observed'
        GROUP BY metadata ->> 'kind'
        ORDER BY 1
        "#,
    )
    .bind(session)
    .fetch_all(&mut **tx)
    .await?)
}
