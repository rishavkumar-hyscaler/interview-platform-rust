//! Authenticate before opening the owner-scoped database transaction.
use crate::{Result, authenticate, digest, infrastructure::interviewer_transaction};
use axum::http::HeaderMap;
use sqlx::{PgPool, Postgres, Transaction};
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
    let token = crate::controllers::auth_controller::bearer_token(headers)?;
    let (recruiter, id) =
        crate::repositories::auth_repository::candidate_for_token(pool, &digest(token))
            .await?
            .ok_or_else(crate::unauthorized)?;
    Ok((
        recruiter,
        id,
        interviewer_transaction(pool, recruiter).await?,
    ))
}
