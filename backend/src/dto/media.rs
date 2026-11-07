use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Deserialize, Serialize)]
pub(crate) struct RecordingResponse {
    pub recording_id: Uuid,
    pub question_id: Uuid,
    pub mime: String,
    pub bytes: u64,
    pub sha256: String,
    pub actor: String,
    pub assessment: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<String>,
}
#[derive(Serialize)]
pub(crate) struct RecordingsResponse {
    pub recordings: Vec<RecordingResponse>,
}
