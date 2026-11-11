//! Invitation/session persistence; encrypted tokens never leave the service except as a recovered link.
use crate::{
    Result,
    models::{
        interview::Session,
        invitation::{InvitationState, InvitationToken},
    },
};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) struct NewInvitation<'a> {
    pub id: Uuid,
    pub owner: Uuid,
    pub job: Uuid,
    pub request_key: Uuid,
    pub name: &'a str,
    pub email: &'a str,
    pub duration: i32,
    pub interviewee: Uuid,
}
pub(crate) async fn job(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    id: Uuid,
) -> Result<Option<Uuid>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM jobs WHERE interviewer_id=$1 AND id=$2")
            .bind(owner)
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?,
    )
}
pub(crate) async fn insert_interviewee(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO users(id,
                email,
                role)
        VALUES ($1,
                $2,
                'interviewee')
        ON CONFLICT (email,
                role) DO NOTHING
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(email)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
pub(crate) async fn interviewee(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
) -> Result<Option<Uuid>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM users WHERE email=$1 AND role='interviewee'")
            .bind(email)
            .fetch_optional(&mut **tx)
            .await?,
    )
}
pub(crate) async fn insert(
    tx: &mut Transaction<'_, Postgres>,
    input: NewInvitation<'_>,
) -> Result<bool> {
    Ok(sqlx::query(
        r#"
        INSERT INTO interview_sessions(id,
                interviewer_id,
                job_id,
                request_key,
                candidate_name,
                candidate_email,
                duration_minutes,
                interviewee_id)
        VALUES ($1,
                $2,
                $3,
                $4,
                $5,
                $6,
                $7,
                $8)
        ON CONFLICT (interviewer_id,
                request_key) DO NOTHING
        "#,
    )
    .bind(input.id)
    .bind(input.owner)
    .bind(input.job)
    .bind(input.request_key)
    .bind(input.name)
    .bind(input.email)
    .bind(input.duration)
    .bind(input.interviewee)
    .execute(&mut **tx)
    .await?
    .rows_affected()
        == 1)
}
pub(crate) async fn by_request(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    key: Uuid,
) -> Result<Session> {
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
            s.interviewee_id
        FROM interview_sessions s
        WHERE interviewer_id = $1
        AND request_key = $2
        "#,
    )
    .bind(owner)
    .bind(key)
    .fetch_one(&mut **tx)
    .await?)
}
pub(crate) async fn insert_token(
    tx: &mut Transaction<'_, Postgres>,
    hash: &str,
    id: Uuid,
    sealed: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO auth_sessions(token_hash,
                user_id,
                interviewer_id,
                interview_session_id,
                expires_at,
                sealed_token)
        SELECT $1,
            interviewee_id,
            interviewer_id,
            id,
            expires_at,
            $3
        FROM interview_sessions
        WHERE id = $2
        "#,
    )
    .bind(hash)
    .bind(id)
    .bind(sealed)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
pub(crate) async fn sealed_token(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner: Uuid,
) -> Result<Option<String>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT sealed_token
        FROM auth_sessions
        WHERE interview_session_id = $1
        AND interviewer_id = $2
        "#,
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut **tx)
    .await?
    .flatten())
}
pub(crate) async fn lock_session(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner: Uuid,
) -> Result<Option<InvitationState>> {
    Ok(sqlx::query_as(
        r#"
        SELECT state,
            expires_at > now()
        AND least(expires_at,
                coalesce(deadline_at,
                    expires_at)) > now() AS active
        FROM interview_sessions
        WHERE id = $1
        AND interviewer_id = $2
        FOR UPDATE
        "#,
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn lock_token(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    owner: Uuid,
) -> Result<Option<InvitationToken>> {
    Ok(sqlx::query_as(
        r#"
        SELECT token_hash,
            sealed_token,
            expires_at::text AS expires_at
        FROM auth_sessions
        WHERE interview_session_id = $1
        AND interviewer_id = $2
        AND expires_at > now()
        FOR UPDATE
        "#,
    )
    .bind(id)
    .bind(owner)
    .fetch_optional(&mut **tx)
    .await?)
}
pub(crate) async fn replace_token(
    tx: &mut Transaction<'_, Postgres>,
    hash: &str,
    sealed: &str,
    id: Uuid,
    owner: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE auth_sessions
        SET token_hash = $1,
            sealed_token = $2
        WHERE interview_session_id = $3
        AND interviewer_id = $4
        "#,
    )
    .bind(hash)
    .bind(sealed)
    .bind(id)
    .bind(owner)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

// Preparation has no invitation deadline. PostgreSQL infinity stays internal;
// the worker replaces it with a real seven-day expiry when publishing the link.
pub(crate) async fn await_preparation(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<()> {
    sqlx::query("UPDATE interview_sessions SET expires_at='infinity'::timestamptz WHERE id=$1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub(crate) async fn unpublished_sessions(
    tx: &mut Transaction<'_, Postgres>,
    set: Uuid,
) -> Result<Vec<Uuid>> {
    Ok(sqlx::query_scalar("SELECT s.id FROM interview_sessions s WHERE question_set_id=$1 AND state='invited' AND NOT EXISTS (SELECT 1 FROM auth_sessions a WHERE a.interview_session_id=s.id) FOR UPDATE")
        .bind(set).fetch_all(&mut **tx).await?)
}
pub(crate) async fn start_invitation_expiry(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<()> {
    sqlx::query(
        "UPDATE interview_sessions SET expires_at=clock_timestamp()+interval '7 days' WHERE id=$1",
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
pub(crate) async fn preparation_complete(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM interview_sessions s JOIN question_sets p ON p.id=s.question_set_id WHERE s.id=$1 AND p.status='completed')")
        .bind(id).fetch_one(&mut **tx).await?)
}
