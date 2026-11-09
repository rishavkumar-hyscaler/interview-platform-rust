//! Authentication HTTP parsing and explicit response contracts.
use crate::{
    Result,
    dto::auth::{LoginRequest, RegisterRequest, SessionResponse, UserResponse},
    models::auth::Session,
    services::auth_service::{self, Credentials},
};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) async fn authenticate(pool: &PgPool, headers: &HeaderMap) -> Result<(Uuid, String)> {
    let token = bearer_token(headers)?;
    auth_service::authenticate_token(pool, token).await
}
pub(crate) async fn register(
    State(pool): State<PgPool>,
    Json(input): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<SessionResponse>)> {
    let session = auth_service::register(
        &pool,
        Credentials {
            email: input.email,
            password: input.password,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(response(session))))
}
pub(crate) async fn login(
    State(pool): State<PgPool>,
    Json(input): Json<LoginRequest>,
) -> Result<Json<SessionResponse>> {
    let session = auth_service::login(
        &pool,
        Credentials {
            email: input.email,
            password: input.password,
        },
    )
    .await?;
    Ok(Json(response(session)))
}
pub(crate) async fn me(
    State(pool): State<PgPool>,
    headers: HeaderMap,
) -> Result<Json<UserResponse>> {
    let (user_id, _) = authenticate(&pool, &headers).await?;
    Ok(Json(UserResponse {
        email: auth_service::interviewer_email(&pool, user_id).await?,
        user_id,
        role: "interviewer",
    }))
}
pub(crate) async fn logout(State(pool): State<PgPool>, headers: HeaderMap) -> Result<StatusCode> {
    let (_, hash) = authenticate(&pool, &headers).await?;
    auth_service::logout(&pool, &hash).await?;
    Ok(StatusCode::NO_CONTENT)
}
fn response(session: Session) -> SessionResponse {
    SessionResponse {
        token: session.token,
        user_id: session.user_id,
        role: "interviewer",
    }
}

pub(crate) fn bearer_token(headers: &HeaderMap) -> Result<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| v.len() == 64)
        .ok_or_else(auth_service::unauthorized)
}
