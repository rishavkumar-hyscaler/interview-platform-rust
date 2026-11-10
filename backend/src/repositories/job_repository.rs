//! Job SQL only. The calling service owns the transaction and business rules.
use crate::{
    Result,
    models::job::{Job, JobListItem, NewJob},
};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub(crate) async fn insert(tx: &mut Transaction<'_, Postgres>, job: &NewJob) -> Result<bool> {
    let result = sqlx::query(
        r#"
        INSERT INTO jobs(id,
                interviewer_id,
                request_key,
                title,
                description,
                job_code,
                preparation_duration_minutes,
                question_set_count)
        VALUES ($1,
                $2,
                $3,
                $4,
                $5,
                $6,
                $7,
                $8)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(job.id)
    .bind(job.interviewer_id)
    .bind(job.request_key)
    .bind(&job.title)
    .bind(&job.description)
    .bind(&job.job_code)
    .bind(job.preparation_duration_minutes)
    .bind(job.question_set_count)
    .execute(&mut **tx)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn find_by_request_key(
    tx: &mut Transaction<'_, Postgres>,
    interviewer: Uuid,
    request_key: Uuid,
) -> Result<Option<Job>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id,
            interviewer_id,
            request_key,
            job_code,
            title,
            description,
            to_jsonb(created_at) #>> '{}' AS created_at,
            preparation_duration_minutes,
            question_set_count
        FROM jobs
        WHERE interviewer_id = $1
        AND request_key = $2
        "#,
    )
    .bind(interviewer)
    .bind(request_key)
    .fetch_optional(&mut **tx)
    .await?)
}

pub(crate) async fn list(
    tx: &mut Transaction<'_, Postgres>,
    interviewer: Uuid,
    cursor: Option<Uuid>,
    fetch_limit: i64,
) -> Result<Vec<JobListItem>> {
    Ok(sqlx::query_as(
        r#"
        SELECT j.id,
            j.interviewer_id,
            j.request_key,
            j.job_code,
            j.title,
            j.description,
            to_jsonb(j.created_at) #>> '{}' AS created_at,
            j.preparation_duration_minutes,
            j.question_set_count,
            (
            SELECT count(*)
            FROM interview_sessions s
            WHERE s.job_id = j.id) AS session_count
        FROM jobs j
        WHERE j.interviewer_id = $1
        AND ($2::uuid IS NULL
            OR (j.created_at,
                    j.id) < (
                SELECT created_at,
                    id
                FROM jobs
                WHERE id = $2))
        ORDER BY j.created_at DESC,
            j.id DESC
        LIMIT $3
        "#,
    )
    .bind(interviewer)
    .bind(cursor)
    .bind(fetch_limit)
    .fetch_all(&mut **tx)
    .await?)
}

pub(crate) async fn delete(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
    job: Uuid,
) -> Result<Vec<Uuid>> {
    let locked_job: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM jobs WHERE interviewer_id = $1 AND id = $2 FOR UPDATE")
            .bind(owner)
            .bind(job)
            .fetch_optional(&mut **tx)
            .await?;
    if locked_job.is_none() {
        return Ok(Vec::new());
    }

    let sessions: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM interview_sessions WHERE interviewer_id = $1 AND job_id = $2 FOR UPDATE",
    )
    .bind(owner)
    .bind(job)
    .fetch_all(&mut **tx)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM audit_events
        WHERE interviewer_id = $1
          AND (
            entity_id = $2 OR entity_id = ANY($3)
            OR entity_id IN (
                SELECT id FROM question_sets WHERE interviewer_id = $1 AND job_id = $2
            )
          )
        "#,
    )
    .bind(owner)
    .bind(job)
    .bind(&sessions)
    .execute(&mut **tx)
    .await?;

    for query in [
        "DELETE FROM auth_sessions WHERE interviewer_id = $1 AND interview_session_id = ANY($2)",
        "DELETE FROM interview_answers WHERE interviewer_id = $1 AND session_id = ANY($2)",
    ] {
        sqlx::query(query)
            .bind(owner)
            .bind(&sessions)
            .execute(&mut **tx)
            .await?;
    }

    for query in [
        "DELETE FROM interview_sessions WHERE interviewer_id = $1 AND job_id = $2",
        "DELETE FROM interview_questions WHERE interviewer_id = $1 AND job_id = $2",
        "DELETE FROM question_sets WHERE interviewer_id = $1 AND job_id = $2",
        "DELETE FROM jobs WHERE interviewer_id = $1 AND id = $2",
    ] {
        sqlx::query(query)
            .bind(owner)
            .bind(job)
            .execute(&mut **tx)
            .await?;
    }

    Ok(sessions)
}
