//! Background persistence only. Services retain transaction and retry boundaries.
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgQueryResult};
use uuid::Uuid;

pub(crate) async fn questions(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Vec<Value>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                q.id,
                'prompt',
                q.prompt,
                'rubric',
                q.rubric,
                'answer',
                coalesce(a.content,
                    ''))
        FROM session_question_details() q
        LEFT JOIN interview_answers a ON a.session_id = q.session_id
        AND a.question_id = q.id
        WHERE q.session_id = $1
        ORDER BY q.position
        "#,
    )
    .bind(session)
    .fetch_all(&mut **tx)
    .await
}

pub(crate) async fn assessment(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Option<(Uuid, Value)>, sqlx::Error> {
    sqlx::query_as(
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
    .await
}

pub(crate) async fn events(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> Result<Vec<(Uuid, Value)>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT id,
            jsonb_build_object('id',
                id,
                'received_at',
                created_at,
                'action',
                action,
                'metadata',
                metadata)
        FROM audit_events
        WHERE entity_id = $1
        AND action IN ('proctor.observed',
                'proctor.evidence',
                'integrity.annotated',
                'session.timed_out',
                'answer.recorded')
        ORDER BY created_at,
            id
        "#,
    )
    .bind(session)
    .fetch_all(&mut **tx)
    .await
}

pub(crate) async fn interviewers(pool: &PgPool) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer' ORDER BY id")
        .fetch_all(pool)
        .await
}

pub(crate) async fn backfill_jobs(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO embedding_jobs(session_id,
                interviewer_id)
        SELECT s.id,
            s.interviewer_id
        FROM interview_sessions s
        WHERE EXISTS (
            SELECT 1
            FROM audit_events a
            WHERE a.entity_id = s.id
            AND a.action = 'assessment.completed')
        AND NOT EXISTS (
            SELECT 1
            FROM embedding_jobs j
            WHERE j.session_id = s.id)
        ON CONFLICT (session_id) DO NOTHING
        "#,
    )
    .execute(&mut **tx)
    .await
}

pub(crate) async fn invalidate_model(
    tx: &mut Transaction<'_, Postgres>,
    model: &str,
    dimensions: i32,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE embedding_jobs
        SET status = 'pending',
            revision = revision + 1,
            attempts = 0,
            last_error = NULL,
            available_at = now(),
            updated_at = now()
        WHERE status IN ('completed',
                'failed')
        AND (embedding_model IS DISTINCT
            FROM $1
            OR embedding_dimensions IS DISTINCT
            FROM $2)
        "#,
    )
    .bind(model)
    .bind(dimensions)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn expire_final_attempts(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE embedding_jobs
        SET status = 'failed',
            claim_token = NULL,
            lease_until = NULL,
            last_error = 'Embedding worker stopped during its final attempt',
            updated_at = now()
        WHERE status = 'running'
        AND lease_until < now()
        AND attempts >= 4
        "#,
    )
    .execute(&mut **tx)
    .await
}

pub(crate) async fn claim(
    tx: &mut Transaction<'_, Postgres>,
    token: Uuid,
    model: &str,
    dimensions: i32,
) -> Result<Option<(Uuid, i64)>, sqlx::Error> {
    sqlx::query_as(
        r#"
        UPDATE embedding_jobs
        SET status = 'running',
            attempts = attempts + 1,
            claim_token = $1,
            lease_until = now() + interval '5 minutes',
            embedding_model = $2,
            embedding_dimensions = $3,
            updated_at = now()
        WHERE session_id = (
            SELECT session_id
            FROM embedding_jobs
            WHERE ((status = 'pending'
                    AND available_at <= now())
                OR (status = 'running'
                    AND lease_until < now()))
            AND attempts < 4
            ORDER BY available_at,
                session_id
            FOR UPDATE SKIP LOCKED
            LIMIT 1)
        RETURNING session_id,
            revision
        "#,
    )
    .bind(token)
    .bind(model)
    .bind(dimensions)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn fail_attempt(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    token: Uuid,
    revision: i64,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(r#"
        UPDATE embedding_jobs
        SET status = CASE WHEN revision <> $3 THEN 'pending' WHEN attempts >= 4 THEN 'failed' ELSE 'pending' END,
            attempts = CASE WHEN revision <> $3 THEN 0 ELSE attempts END,
            last_error = CASE WHEN revision <> $3 THEN NULL ELSE 'Embedding request or index write failed; retry available' END,
            available_at = CASE WHEN revision <> $3 THEN greatest(available_at,
                now() + interval '2 seconds') ELSE now() + attempts * interval '5 seconds' END,
            claim_token = NULL,
            lease_until = NULL,
            updated_at = now()
        WHERE session_id = $1
        AND claim_token = $2
        AND status = 'running'
        "#).bind(session).bind(token).bind(revision).execute(&mut **tx).await
}

pub(crate) async fn lock_revision(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    token: Uuid,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT revision
        FROM embedding_jobs
        WHERE session_id = $1
        AND claim_token = $2
        AND status = 'running'
        AND lease_until > now()
        FOR UPDATE
        "#,
    )
    .bind(session)
    .bind(token)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn delete_stale_chunks(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    questions: &[Uuid],
    model: &str,
    dimensions: i32,
    ids: &[String],
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        DELETE
        FROM evidence_embeddings
        WHERE (session_id = $1
            OR question_id = ANY ($2))
        AND embedding_model = $3
        AND embedding_dimensions = $4
        AND NOT (source_id = ANY ($5))
        "#,
    )
    .bind(session)
    .bind(questions)
    .bind(model)
    .bind(dimensions)
    .bind(ids)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn complete(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    token: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE embedding_jobs
        SET status = 'completed',
            indexed_revision = revision,
            last_error = NULL,
            claim_token = NULL,
            lease_until = NULL,
            updated_at = now()
        WHERE session_id = $1
        AND claim_token = $2
        "#,
    )
    .bind(session)
    .bind(token)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn cached_vectors(
    tx: &mut Transaction<'_, Postgres>,
    hashes: &[String],
    model: &str,
    dimensions: i32,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    sqlx::query_as(
        r#"
        SELECT content_hash,
            embedding::text
        FROM evidence_embeddings
        WHERE content_hash = ANY ($1)
        AND embedding_model = $2
        AND embedding_dimensions = $3
        "#,
    )
    .bind(hashes)
    .bind(model)
    .bind(dimensions)
    .fetch_all(&mut **tx)
    .await
}

pub(crate) async fn upsert_chunk(
    tx: &mut Transaction<'_, Postgres>,
    input: crate::models::rag::EvidenceEmbeddingInsert<'_>,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO evidence_embeddings(interviewer_id,
                session_id,
                question_id,
                source_id,
                source_type,
                content,
                content_hash,
                embedding,
                embedding_model,
                embedding_dimensions)
        VALUES ($1,
                $2,
                $3,
                $4,
                $5,
                $6,
                $7,
                $8::vector,
                $9,
                $10)
        ON CONFLICT (interviewer_id,
                scope_id,
                source_id,
                embedding_model,
                embedding_dimensions)
        DO UPDATE
        SET content = excluded.content,
            content_hash = excluded.content_hash,
            embedding = excluded.embedding,
            updated_at = now()
        WHERE evidence_embeddings.content_hash <> excluded.content_hash
        "#,
    )
    .bind(input.owner)
    .bind(input.session)
    .bind(input.question)
    .bind(input.source_id)
    .bind(input.kind)
    .bind(input.content)
    .bind(input.hash)
    .bind(input.vector)
    .bind(input.model)
    .bind(input.dimensions)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn renew_lease(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    token: Uuid,
) -> Result<PgQueryResult, sqlx::Error> {
    sqlx::query(
        r#"
        UPDATE embedding_jobs
        SET lease_until = now() + interval '5 minutes',
            updated_at = now()
        WHERE session_id = $1
        AND claim_token = $2
        "#,
    )
    .bind(session)
    .bind(token)
    .execute(&mut **tx)
    .await
}

pub(crate) async fn ready(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    model: &str,
    dimensions: i32,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM embedding_jobs
            WHERE session_id = $1
            AND status = 'completed'
            AND revision = indexed_revision
            AND embedding_model = $2
            AND embedding_dimensions = $3)
        "#,
    )
    .bind(session)
    .bind(model)
    .bind(dimensions)
    .fetch_one(&mut **tx)
    .await
}

pub(crate) async fn lock_current_index(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    model: &str,
    dimensions: i32,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        r#"
        SELECT session_id
        FROM embedding_jobs
        WHERE session_id = $1
        AND status = 'completed'
        AND revision = indexed_revision
        AND embedding_model = $2
        AND embedding_dimensions = $3
        FOR SHARE
        "#,
    )
    .bind(session)
    .bind(model)
    .bind(dimensions)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn search(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    model: &str,
    dimensions: i32,
    vector: String,
    offset: i64,
) -> Result<Vec<(String, String)>, sqlx::Error> {
    sqlx::query_as(&format!(
        r#"
        WITH eligible AS MATERIALIZED ({ELIGIBLE})
        SELECT source_id, content
        FROM eligible
        ORDER BY embedding <=> $4::vector, source_id
        OFFSET $5
        LIMIT 6
        "#
    ))
    .bind(session)
    .bind(model)
    .bind(dimensions)
    .bind(vector)
    .bind(offset)
    .fetch_all(&mut **tx)
    .await
}

pub(crate) async fn source_count(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
    model: &str,
    dimensions: i32,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(&format!("SELECT count(*) FROM ({ELIGIBLE}) scoped"))
        .bind(session)
        .bind(model)
        .bind(dimensions)
        .fetch_one(&mut **tx)
        .await
}

// Both semantic retrieval queries use the same session/set boundary.
const ELIGIBLE: &str = r#"
    SELECT e.source_id, e.content, e.embedding
    FROM evidence_embeddings e
    WHERE e.embedding_model = $2
        AND e.embedding_dimensions = $3
        AND (
            e.session_id = $1
            OR (
                e.session_id IS NULL
                AND EXISTS (
                    SELECT 1
                    FROM session_question_details() q
                    WHERE q.session_id = $1
                        AND q.id = e.question_id
                )
            )
        )
"#;
