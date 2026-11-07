//! Allowlisted generation API response fields. Nested AI schemas remain JSON documents.
use serde::Serialize;
use serde_json::Value;
#[derive(Serialize)]
pub(crate) struct SessionGenerationResponse {
    pub max_attempts: i32,
    pub status: String,
    pub configured: bool,
    pub job: Option<Value>,
    pub blueprint: Option<Value>,
    pub questions: Vec<Value>,
    pub prompt_version: &'static str,
}
#[derive(Serialize)]
pub(crate) struct QuestionSetsResponse {
    pub max_attempts: i32,
    pub configured: bool,
    pub target: i32,
    pub duration_minutes: i32,
    pub blueprint: Option<Value>,
    pub sets: Vec<Value>,
    pub questions: Vec<Value>,
}
#[derive(Serialize)]
pub(crate) struct PendingResponse {
    pub status: &'static str,
}
