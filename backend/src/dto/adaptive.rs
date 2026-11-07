use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecommendationRequest {
    pub question_id: Uuid,
}
#[derive(Serialize)]
pub(crate) struct RecommendationResponse {
    pub position: Option<i32>,
}
