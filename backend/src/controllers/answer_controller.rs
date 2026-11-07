use crate::{
    Result,
    access::candidate,
    dto::answer::*,
    services::answer_service::{self, SaveAnswer},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
};
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) fn routes() -> Router<PgPool> {
    Router::new()
        .route("/api/v1/candidate/room", get(room))
        .route("/api/v1/candidate/answers/{question}", post(save))
}
async fn room(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(query): Query<RoomQuery>,
) -> Result<Json<RoomResponse>> {
    let (owner, id, tx) = candidate(&pool, &headers).await?;
    let result = answer_service::room(owner, id, tx, query.position).await?;
    Ok(Json(RoomResponse {
        session: super::candidate_controller::response(result.session),
        question: result
            .question
            .map(|q| QuestionResponse {
                id: q.id,
                position: q.position,
                prompt: q.prompt,
                kind: q.kind,
                difficulty: q.difficulty,
                answer: q.answer,
                revision: q.revision,
                saved_at: q.saved_at,
            }),
        question_started_at: result.question_started_at,
    }))
}
async fn save(
    State(pool): State<PgPool>,
    Path(question): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<SaveAnswerRequest>,
) -> Result<Json<SavedAnswerResponse>> {
    let (owner, id, tx) = candidate(&pool, &headers).await?;
    let result = answer_service::save(
        owner,
        id,
        tx,
        question,
        SaveAnswer {
            content: input.content,
            revision: input.revision,
        },
    )
    .await?;
    Ok(Json(SavedAnswerResponse {
        question_id: result.question_id,
        revision: result.revision,
        saved_at: result.saved_at,
        answer_count: result.answer_count,
    }))
}
