//! Authenticate before opening the owner-scoped database transaction.
use crate::{Result, authenticate, digest, infrastructure::interviewer_transaction};
use axum::http::HeaderMap;
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;
pub(crate) async fn staff<'a>(
    pool: &'a PgPool,
    headers: &HeaderMap,
) -> Result<(Uuid, Transaction<'a, Postgres>)> {
    let (user, _) = authenticate(pool, headers).await?;
    Ok((user, interviewer_transaction(pool, user).await?))
}
pub(crate) async fn candidate<'a>(
    pool: &'a PgPool,
    headers: &HeaderMap,
) -> Result<(Uuid, Uuid, Transaction<'a, Postgres>)> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| v.len() == 64)
        .ok_or_else(crate::unauthorized)?;
    let row=sqlx::query("SELECT a.interviewer_id,a.interview_session_id AS session_id FROM auth_sessions a JOIN users u ON u.id=a.user_id WHERE a.token_hash=$1 AND a.expires_at>now() AND a.interview_session_id IS NOT NULL AND u.role='interviewee'")
        .bind(digest(token)).fetch_optional(pool).await?.ok_or_else(crate::unauthorized)?;
    let recruiter: Uuid = row.get("interviewer_id");
    let id: Uuid = row.get("session_id");
    Ok((
        recruiter,
        id,
        interviewer_transaction(pool, recruiter).await?,
    ))
}
