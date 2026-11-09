use super::candidate::CandidateSessionResponse;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct RoomQuery {
    pub position: Option<i32>,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SaveAnswerRequest {
    pub content: String,
    pub revision: i32,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct QuestionResponse {
    pub id: Uuid,
    pub position: i32,
    pub prompt: String,
    pub kind: String,
    pub difficulty: String,
    pub answer: String,
    pub revision: i32,
    pub saved_at: Option<String>,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct SavedAnswerResponse {
    pub question_id: Uuid,
    pub revision: i32,
    pub saved_at: String,
    pub answer_count: i64,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct RoomResponse {
    pub session: CandidateSessionResponse,
    pub question: Option<QuestionResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question_started_at: Option<String>,
}
