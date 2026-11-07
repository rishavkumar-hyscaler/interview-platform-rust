use serde::Serialize;
use serde_json::Value;
#[derive(Serialize)]
pub(crate) struct AssessmentResponse {
    pub configured: bool,
    pub state: String,
    pub assessment: Option<Value>,
    pub attempts: i64,
    pub max_attempts: i32,
    pub last_failure: Option<Value>,
    pub failed: bool,
    pub observations: Vec<Value>,
    pub integrity_notice: &'static str,
}
#[derive(Serialize)]
pub(crate) struct RetryResponse {
    pub queued: bool,
}
