//! Allowlisted session responses. Never serialize a database row directly.
use serde::Serialize;
use uuid::Uuid;
#[derive(Serialize)]
pub(crate) struct SessionResponse {
    pub id: Uuid,
    pub interviewer_id: Uuid,
    pub job_id: Uuid,
    pub question_set_id: Option<Uuid>,
    pub candidate_name: String,
    pub candidate_email: String,
    pub state: String,
    pub duration_minutes: i32,
    pub expires_at: String,
    pub started_at: Option<String>,
    pub deadline_at: Option<String>,
    pub submitted_at: Option<String>,
    pub created_at: String,
    pub interviewee_id: Uuid,
}
#[derive(Serialize)]
pub(crate) struct SessionListItemResponse {
    #[serde(flatten)]
    pub session: SessionResponse,
    pub job_title: String,
    pub job_code: String,
}
#[derive(Serialize)]
pub(crate) struct SessionDetailResponse {
    #[serde(flatten)]
    pub item: SessionListItemResponse,
    pub description: String,
    pub question_count: i64,
    pub answer_count: i64,
    pub invitation_recoverable: bool,
    pub invitation_active: bool,
}
#[derive(Serialize)]
pub(crate) struct SessionListResponse {
    pub items: Vec<SessionListItemResponse>,
    pub next_cursor: Option<Uuid>,
}
#[derive(Serialize)]
pub(crate) struct SummaryResponse {
    pub jobs: i64,
    pub sessions: i64,
    pub active: i64,
    pub reviewable: i64,
}
