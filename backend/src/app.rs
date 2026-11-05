//! Application composition, health checks and HTTP-wide policies.
use crate::{
    Result, adaptive,
    auth::{login, logout, me, register},
    generation, interview_room, interviews, media, proctoring, review, scoring,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use sqlx::PgPool;
pub fn router(pool: PgPool) -> Router {
    router_with_ai(
        pool,
        generation::provider::AiProvider::from_environment().ok(),
    )
}
pub fn router_with_ai(pool: PgPool, provider: Option<generation::provider::AiProvider>) -> Router {
    let automatic = generation::AutomaticPreparation(provider.is_some());
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"status": "ok"})) }),
        )
        .route("/health/ready", get(ready))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/me", get(me))
        .merge(interviews::routes())
        .merge(interview_room::routes())
        .merge(media::routes())
        .merge(proctoring::routes())
        .merge(crate::proctor_evidence::routes())
        .merge(scoring::routes())
        .merge(review::routes())
        .merge(adaptive::routes())
        .merge(generation::routes(provider))
        .layer(
            tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            ),
        )
        .layer(axum::Extension(automatic))
        .layer(DefaultBodyLimit::max(512 * 1024))
        .layer(axum::middleware::map_response(normalize_errors))
        .layer(axum::middleware::from_fn(crate::observability::request_log))
        .with_state(pool)
}
// Normalize extractor/routing failures without echoing rejected input or secrets.
async fn normalize_errors(response: Response) -> Response {
    if response.status().is_client_error()
        && !response
            .headers()
            .get("content-type")
            .is_some_and(|v| v.as_bytes().starts_with(b"application/json"))
    {
        return (response.status(), Json(json!({"error":"Invalid request, route, or request body", "code":"invalid_request"}))).into_response();
    }
    response
}
async fn ready(State(pool): State<PgPool>) -> Result<Json<Value>> {
    sqlx::query("SELECT 1").execute(&pool).await?;
    Ok(Json(json!({"status": "ready"})))
}
