//! Answer and question SQL. The candidate service holds the session lock.
use crate::{
    Result,
    models::answer::{CurrentAnswer, Question, SavedAnswer},
};
use serde_json::Value;
use sqlx::{Postgres, Transaction, types::Json};
use uuid::Uuid;
pub(crate) async fn lock_state(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<String> {
    Ok(
        sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1 FOR UPDATE")
            .bind(id)
            .fetch_one(&mut **tx)
            .await?,
    )
}
pub(crate) async fn previous(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<Value>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT metadata
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'question.served'
        ORDER BY created_at DESC,
            id DESC
        LIMIT 1
        "#,
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn question(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    position: i32,
) -> Result<Option<Question>> {
    let row: Option<Json<Question>> = sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('id',
                q.id,
                'position',
                q.position,
                'prompt',
                q.prompt,
                'kind',
                q.kind,
                'difficulty',
                q.difficulty,
                'answer',
                coalesce(a.content,
                    ''),
                'revision',
                coalesce(a.revision,
                    0),
                'saved_at',
                a.updated_at)
        FROM session_question_details() q
        LEFT JOIN interview_answers a ON a.question_id = q.id
        AND a.session_id = q.session_id
        WHERE q.session_id = $1
        AND q.position = $2
        "#,
    )
    .bind(id)
    .bind(position)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|v| v.0))
}
pub(crate) async fn served_at(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    position: i32,
) -> Result<String> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT to_jsonb(created_at) #>> '{}'
        FROM audit_events
        WHERE entity_id = $1
        AND action = 'question.served'
        AND metadata ->> 'position' = $2
        ORDER BY created_at DESC,
            id DESC
        LIMIT 1
        "#,
    )
    .bind(id)
    .bind(position.to_string())
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn question_exists(
    tx: &mut Transaction<'_, Postgres>,
    question: Uuid,
    id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM session_question_details()
            WHERE id = $1
            AND session_id = $2)
        "#,
    )
    .bind(question)
    .bind(id)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn current(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    question: Uuid,
) -> Result<Option<CurrentAnswer>> {
    Ok(sqlx::query_as(
        r#"
        SELECT content,
            revision
        FROM interview_answers
        WHERE session_id = $1
        AND question_id = $2
        FOR UPDATE
        "#,
    )
    .bind(id)
    .bind(question)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn save(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    id: Uuid,
    question: Uuid,
    content: &str,
    revision: i32,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO interview_answers(id,
                interviewer_id,
                session_id,
                question_id,
                content,
                revision)
        VALUES ($1,
                $2,
                $3,
                $4,
                $5,
                $6)
        ON CONFLICT (session_id,
                question_id)
        DO UPDATE
        SET content = excluded.content,
            revision = excluded.revision,
            updated_at = now()
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(owner)
    .bind(id)
    .bind(question)
    .bind(content)
    .bind(revision)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
pub(crate) async fn saved(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    question: Uuid,
) -> Result<SavedAnswer> {
    let value: Json<SavedAnswer> = sqlx::query_scalar(
        r#"
        SELECT jsonb_build_object('question_id',
                question_id,
                'revision',
                revision,
                'saved_at',
                updated_at,
                'answer_count',
                (
                SELECT count(*)
                FROM session_question_details() q
                WHERE q.session_id = $1
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
                        AND m.metadata ->> 'question_id' = q.id::text))))
        FROM interview_answers
        WHERE session_id = $1
        AND question_id = $2
        "#,
    )
    .bind(id)
    .bind(question)
    .fetch_one(&mut **tx)
    .await?;
    Ok(value.0)
}
