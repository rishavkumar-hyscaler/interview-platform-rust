//! Authentication queries. Credential hashes remain internal.
use crate::{Result, models::auth::Credentials};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn create_user(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
    email: &str,
    hash: &str,
) -> Result<bool> {
    Ok(sqlx::query(
        r#"
        INSERT INTO users(id,
                email,
                password_hash)
        VALUES ($1,
                $2,
                $3)
        ON CONFLICT (email,
                role) DO NOTHING
        "#,
    )
    .bind(user)
    .bind(email)
    .bind(hash)
    .execute(&mut **tx)
    .await?
    .rows_affected()
        == 1)
}
pub(crate) async fn insert_session(
    tx: &mut Transaction<'_, Postgres>,
    hash: &str,
    user: Uuid,
) -> Result<()> {
    sqlx::query("INSERT INTO auth_sessions(token_hash,user_id) VALUES ($1,$2)")
        .bind(hash)
        .bind(user)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
pub(crate) async fn record_registration(
    tx: &mut Transaction<'_, Postgres>,
    user: Uuid,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO audit_events(id,
                interviewer_id,
                actor_id,
                action)
        VALUES ($1,
                $2,
                $3,
                'recruiter.registered')
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(user)
    .bind(user)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
pub(crate) async fn credentials(pool: &PgPool, email: &str) -> Result<Option<Credentials>> {
    Ok(sqlx::query_as(
        r#"
        SELECT id,
            password_hash
        FROM users
        WHERE email = $1
        AND role = 'interviewer'
        "#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await?)
}
pub(crate) async fn login_session(pool: &PgPool, hash: &str, user: Uuid) -> Result<()> {
    sqlx::query("INSERT INTO auth_sessions(token_hash,user_id) VALUES ($1,$2)")
        .bind(hash)
        .bind(user)
        .execute(pool)
        .await?;
    Ok(())
}
pub(crate) async fn user_for_token(pool: &PgPool, hash: &str) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar(
        r#"
        SELECT s.user_id
        FROM auth_sessions s JOIN users u ON u.id = s.user_id
        WHERE s.token_hash = $1
        AND s.expires_at > now()
        AND u.role = 'interviewer'
        AND s.interview_session_id IS NULL
        "#,
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?)
}
pub(crate) async fn delete_session(pool: &PgPool, hash: &str) -> Result<()> {
    sqlx::query("DELETE FROM auth_sessions WHERE token_hash=$1")
        .bind(hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub(crate) async fn candidate_for_token(pool: &PgPool, hash: &str) -> Result<Option<(Uuid, Uuid)>> {
    Ok(sqlx::query_as(
        r#"
        SELECT a.interviewer_id,
            a.interview_session_id
        FROM auth_sessions a JOIN users u ON u.id = a.user_id
        WHERE a.token_hash = $1
        AND a.expires_at > now()
        AND a.interview_session_id IS NOT NULL
        AND u.role = 'interviewee'
        "#,
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?)
}
