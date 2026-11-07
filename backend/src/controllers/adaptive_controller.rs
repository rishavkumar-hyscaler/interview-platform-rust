use crate::{
    Result,
    access::candidate,
    dto::adaptive::{RecommendationRequest, RecommendationResponse},
    services::adaptive_service,
};
use axum::{Json, Router, extract::State, http::HeaderMap, routing::post};
use sqlx::PgPool;
pub(crate) fn routes() -> Router<PgPool> {
    Router::new().route("/api/v1/candidate/recommendation", post(next))
}
async fn next(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(input): Json<RecommendationRequest>,
) -> Result<Json<RecommendationResponse>> {
    let (owner, session, tx) = candidate(&pool, &headers).await?;
    let session_hash = crate::digest(super::auth_controller::bearer_token(&headers)?);
    let position =
        adaptive_service::next(&pool, owner, session, tx, input.question_id, &session_hash).await?;
    Ok(Json(RecommendationResponse {
        position,
    }))
}
