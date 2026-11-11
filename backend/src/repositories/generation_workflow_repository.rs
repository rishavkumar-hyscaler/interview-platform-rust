//! Background persistence only. Services retain transaction and retry boundaries.
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgQueryResult};
use uuid::Uuid;

pub(crate) async fn insert_set(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner: Uuid,
    job: Uuid,
    duration: i32,
    seed: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO question_sets(id,
                interviewer_id,
                job_id,
                duration_minutes,
                seed)
        VALUES ($1,
                $2,
                $3,
                $4,
                $5)
        "#,
    )
    .bind(id)
    .bind(owner)
    .bind(job)
    .bind(duration)
    .bind(seed)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn assignment_candidate(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Option<crate::models::generation::AssignmentCandidate>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT job_id,
            duration_minutes
        FROM interview_sessions s
        WHERE id = $1
        AND state IN ('invited',
                'ready')
        AND expires_at > clock_timestamp()
        AND s.question_set_id IS NULL
        AND NOT EXISTS (
            SELECT 1
            FROM session_question_details() q
            WHERE q.session_id = s.id)
        FOR UPDATE
        "#,
    )
    .bind(session)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn lock_job(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM jobs WHERE id=$1 FOR UPDATE")
        .bind(job)
        .fetch_one(&mut **tx)
        .await
}

pub(crate) async fn assign_set(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    set: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query("UPDATE interview_sessions SET question_set_id=$2 WHERE id=$1")
        .bind(session)
        .bind(set)
        .execute(&mut **tx)
        .await
}

pub(crate) async fn record_assignment(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner: Uuid,
    session: Uuid,
    metadata: Value,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO audit_events(id,
                interviewer_id,
                entity_id,
                action,
                metadata)
        VALUES ($1,
                $2,
                $3,
                'questions.assigned',
                $4)
        "#,
    )
    .bind(id)
    .bind(owner)
    .bind(session)
    .bind(metadata)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn interviewers(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer' ORDER BY id")
        .fetch_all(pool)
        .await
}

pub(crate) async fn expire_final_attempts(
    tx: &mut Transaction<'_, Postgres>,
    max_attempts: i32,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE question_sets
        SET status = 'failed',
            last_error = 'Generation stopped before its final attempt completed',
            claim_token = NULL,
            lease_until = NULL
        WHERE status = 'running'
        AND lease_until < now()
        AND attempts >= $1
        "#,
    )
    .bind(max_attempts)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn queue_candidate(
    tx: &mut Transaction<'_, Postgres>,
    max_attempts: i32,
) -> Result<Option<crate::models::generation::QueueCandidate>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT p.id,
            p.attempts,
            (extract(epoch
                from j.created_at) * 1000000)::bigint AS job_created,
            (extract(epoch
                from p.created_at) * 1000000)::bigint AS created
        FROM question_sets p JOIN jobs j ON j.id = p.job_id
        WHERE ((p.status = 'pending'
                AND p.available_at <= now())
            OR (p.status = 'running'
                AND p.lease_until < now()))
        AND p.attempts < $1
        ORDER BY p.attempts,
            j.created_at DESC,
            p.created_at,
            p.id
        LIMIT 1
        "#,
    )
    .bind(max_attempts)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn claim_set(
    tx: &mut Transaction<'_, Postgres>,
    token: Uuid,
    set: Uuid,
    max_attempts: i32,
) -> Result<Option<crate::models::generation::ClaimedSet>, sqlx::Error> {
    sqlx::query_as(
        r#"
        UPDATE question_sets
        SET status = 'running',
            claim_token = $1,
            lease_until = now() + interval '5 minutes',
            attempts = attempts + 1
        WHERE id = (
            SELECT id
            FROM question_sets
            WHERE id = $2
            AND ((status = 'pending'
                    AND available_at <= now())
                OR (status = 'running'
                    AND lease_until < now()))
            AND attempts < $3
            FOR UPDATE SKIP LOCKED)
        RETURNING id,
            seed,
            attempts
        "#,
    )
    .bind(token)
    .bind(set)
    .bind(max_attempts)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn fail_attempt(
    tx: &mut Transaction<'_, Postgres>,
    set: Uuid,
    token: Uuid,
    message: String,
    max_attempts: i32,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE question_sets
        SET status = CASE WHEN attempts >= $4 THEN 'failed' ELSE 'pending' END,
            last_error = $3,
            available_at = now() + attempts * interval '5 seconds',
            claim_token = NULL,
            lease_until = NULL
        WHERE id = $1
        AND claim_token = $2
        AND status = 'running'
        "#,
    )
    .bind(set)
    .bind(token)
    .bind(message)
    .bind(max_attempts)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn renew_lease(
    tx: &mut Transaction<'_, Postgres>,
    set: Uuid,
    token: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE question_sets
        SET lease_until = now() + interval '5 minutes'
        WHERE id = $1
        AND claim_token = $2
        AND status = 'running'
        AND lease_until > now()
        "#,
    )
    .bind(set)
    .bind(token)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn preparation_job(
    tx: &mut Transaction<'_, Postgres>,
    set: Uuid,
) -> Result<Option<crate::models::generation::PreparationJob>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT p.job_id,
            j.description,
            p.duration_minutes,
            p.last_error AS generation_error
        FROM question_sets p JOIN jobs j ON j.id = p.job_id
        WHERE p.id = $1
        "#,
    )
    .bind(set)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn blueprint(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT blueprint FROM jobs WHERE id=$1")
        .bind(job)
        .fetch_one(&mut **tx)
        .await
}

pub(crate) async fn recent_prompts(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT q.prompt
        FROM interview_questions q
        WHERE q.job_id = $1
        ORDER BY q.id DESC
        LIMIT 30
        "#,
    )
    .bind(job)
    .fetch_all(&mut **tx)
    .await
}

pub(crate) async fn lock_blueprint(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT blueprint FROM jobs WHERE id=$1 FOR UPDATE")
        .bind(job)
        .fetch_one(&mut **tx)
        .await
}

pub(crate) async fn save_blueprint(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
    blueprint: Value,
    model: &str,
    prompt_version: &str,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE jobs
        SET blueprint = $2,
            blueprint_model = $3,
            blueprint_prompt_version = $4
        WHERE id = $1
        "#,
    )
    .bind(job)
    .bind(blueprint)
    .bind(model)
    .bind(prompt_version)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn saved_question_times(
    tx: &mut Transaction<'_, Postgres>,
    set: Uuid,
) -> Result<Vec<crate::models::generation::SavedQuestionTime>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT position,
            coalesce((rubric ->> 'estimated_minutes')::integer,
                1) AS minutes
        FROM interview_questions
        WHERE set_id = $1
        ORDER BY position
        "#,
    )
    .bind(set)
    .fetch_all(&mut **tx)
    .await
}

pub(crate) async fn plan(
    tx: &mut Transaction<'_, Postgres>,
    set: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT metadata -> 'plan'
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'generation.planned'
        ORDER BY created_at,
            id
        LIMIT 1
        "#,
    )
    .bind(set)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn record_plan(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner: Uuid,
    set: Uuid,
    metadata: Value,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO audit_events(id,
                interviewer_id,
                entity_id,
                action,
                metadata)
        VALUES ($1,
                $2,
                $3,
                'generation.planned',
                $4)
        "#,
    )
    .bind(id)
    .bind(owner)
    .bind(set)
    .bind(metadata)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn lock_owned_set(
    tx: &mut Transaction<'_, Postgres>,
    set: Uuid,
    token: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT id
        FROM question_sets
        WHERE id = $1
        AND claim_token = $2
        AND status = 'running'
        AND lease_until > clock_timestamp()
        FOR UPDATE
        "#,
    )
    .bind(set)
    .bind(token)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn lock_question_bank(
    tx: &mut Transaction<'_, Postgres>,
    job: String,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(job)
        .execute(&mut **tx)
        .await
}

pub(crate) async fn question_count(
    tx: &mut Transaction<'_, Postgres>,
    set: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM interview_questions WHERE set_id=$1")
        .bind(set)
        .fetch_one(&mut **tx)
        .await
}

pub(crate) async fn duplicate_exists(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
    fingerprint: &str,
    model: &str,
    dimensions: i32,
    vector: &str,
    similarity: f64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM interview_questions
            WHERE job_id = $1
            AND (fingerprint = $2
                OR (embedding_model = $3
                    AND embedding_dimensions = $4
                    AND 1 - (embedding <=> $5::vector) >= $6)))
        "#,
    )
    .bind(job)
    .bind(fingerprint)
    .bind(model)
    .bind(dimensions)
    .bind(vector)
    .bind(similarity)
    .fetch_one(&mut **tx)
    .await
}

pub(crate) async fn insert_question(
    tx: &mut Transaction<'_, Postgres>,
    input: crate::models::generation::PreparedQuestionInsert<'_>,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO interview_questions(id,
                interviewer_id,
                job_id,
                position,
                kind,
                difficulty,
                prompt,
                rubric,
                competency_id,
                fingerprint,
                embedding,
                embedding_model,
                embedding_dimensions,
                set_id)
        VALUES ($1,
                $2,
                $3,
                $4,
                $5,
                $6,
                $7,
                $8,
                $9,
                $10,
                $11::vector,
                $12,
                $13,
                $14)
        "#,
    )
    .bind(input.id)
    .bind(input.owner)
    .bind(input.job)
    .bind(input.position)
    .bind(input.kind)
    .bind(input.difficulty)
    .bind(input.prompt)
    .bind(input.rubric)
    .bind(input.competency)
    .bind(input.fingerprint)
    .bind(input.vector)
    .bind(input.model)
    .bind(input.dimensions)
    .bind(input.set)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn advance_checkpoint(
    tx: &mut Transaction<'_, Postgres>,
    set: Uuid,
    token: Uuid,
    complete: bool,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE question_sets
        SET status = CASE WHEN $3 THEN 'completed' ELSE 'running' END,
            claim_token = CASE WHEN $3 THEN NULL ELSE claim_token END,
            lease_until = CASE WHEN $3 THEN NULL ELSE lease_until END,
            last_error = NULL
        WHERE id = $1
        AND claim_token = $2
        "#,
    )
    .bind(set)
    .bind(token)
    .bind(complete)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn record_question(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner: Uuid,
    set: Uuid,
    metadata: Value,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO audit_events(id,
                interviewer_id,
                entity_id,
                action,
                metadata)
        VALUES ($1,
                $2,
                $3,
                'question.prepared',
                $4)
        "#,
    )
    .bind(id)
    .bind(owner)
    .bind(set)
    .bind(metadata)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn record_completion(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner: Uuid,
    set: Uuid,
    metadata: Value,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO audit_events(id,
                interviewer_id,
                entity_id,
                action,
                metadata)
        VALUES ($1,
                $2,
                $3,
                'questions.prepared',
                $4)
        "#,
    )
    .bind(id)
    .bind(owner)
    .bind(set)
    .bind(metadata)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn previous_rejection(
    tx: &mut Transaction<'_, Postgres>,
    set: Uuid,
    position: String,
) -> Result<Option<Value>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT metadata
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'generation.rejected'
        AND metadata ->> 'question_position' = $2
        ORDER BY created_at DESC,
            id DESC
        LIMIT 1
        "#,
    )
    .bind(set)
    .bind(position)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn incompatible_embeddings(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
    model: &str,
    dimensions: i32,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM interview_questions
            WHERE job_id = $1
            AND (embedding_model <> $2
                OR embedding_dimensions <> $3))
        "#,
    )
    .bind(job)
    .bind(model)
    .bind(dimensions)
    .fetch_one(&mut **tx)
    .await
}

pub(crate) async fn exact_collision(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
    fingerprint: String,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT prompt
        FROM interview_questions
        WHERE job_id = $1
        AND fingerprint = $2
        LIMIT 1
        "#,
    )
    .bind(job)
    .bind(fingerprint)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn semantic_collision(
    tx: &mut Transaction<'_, Postgres>,
    job: Uuid,
    fingerprint: String,
    model: &str,
    dimensions: i32,
    vector: String,
    similarity: f64,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT prompt
        FROM interview_questions
        WHERE job_id = $1
        AND (fingerprint = $2
            OR (embedding_model = $3
                AND embedding_dimensions = $4
                AND 1 - (embedding <=> $5::vector) >= $6))
        ORDER BY id
        LIMIT 1
        "#,
    )
    .bind(job)
    .bind(fingerprint)
    .bind(model)
    .bind(dimensions)
    .bind(vector)
    .bind(similarity)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn record_rejection(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner: Uuid,
    set: Uuid,
    metadata: Value,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO audit_events(id,
                interviewer_id,
                entity_id,
                action,
                metadata)
        VALUES ($1,
                $2,
                $3,
                'generation.rejected',
                $4)
        "#,
    )
    .bind(id)
    .bind(owner)
    .bind(set)
    .bind(metadata)
    .execute(&mut **tx)
    .await
}
