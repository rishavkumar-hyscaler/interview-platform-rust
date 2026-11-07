//! Explicit HTTP contracts. Database-only fields never serialize through these DTOs.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize)]
pub(crate) struct CreateJobRequest {
    pub job_code: String,
    pub title: String,
    pub description: String,
    pub request_key: Uuid,
    #[serde(default = "default_duration")]
    pub preparation_duration_minutes: i32,
    #[serde(default = "default_set_count")]
    pub question_set_count: i32,
}
fn default_set_count() -> i32 {
    1
}
fn default_duration() -> i32 {
    30
}

#[derive(Serialize)]
pub(crate) struct JobResponse {
    pub id: Uuid,
    pub job_code: String,
    pub title: String,
    pub description: String,
    pub created_at: String,
    pub preparation_duration_minutes: i32,
    pub question_set_count: i32,
}
#[derive(Serialize)]
pub(crate) struct JobListItemResponse {
    #[serde(flatten)]
    pub job: JobResponse,
    pub session_count: i64,
}
#[derive(Serialize)]
pub(crate) struct JobListResponse {
    pub items: Vec<JobListItemResponse>,
    pub next_cursor: Option<Uuid>,
}
