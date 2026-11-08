//! Authenticated signaling only; media flows between browsers.
use crate::{
    Result, access,
    dto::live_screen::{Signal, View},
};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) async fn staff(
    State(pool): State<PgPool>,
    Path(session): Path<Uuid>,
    headers: HeaderMap,
    Json(signal): Json<Signal>,
) -> Result<Json<View>> {
    let (owner, mut tx) = access::staff(&pool, &headers).await?;
    crate::repositories::live_screen_repository::authorize(&mut tx, owner, session).await?;
    Ok(Json(crate::services::live_screen_service::exchange(
        session, signal, false,
    )?))
}
pub(crate) async fn candidate(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(signal): Json<Signal>,
) -> Result<Json<View>> {
    let (owner, session, mut tx) = access::candidate(&pool, &headers).await?;
    crate::repositories::live_screen_repository::authorize(&mut tx, owner, session).await?;
    Ok(Json(crate::services::live_screen_service::exchange(
        session, signal, true,
    )?))
}
