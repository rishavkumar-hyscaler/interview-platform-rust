//! Candidate-safe query projections, not API serialization types.
use serde::Deserialize;
use uuid::Uuid;
#[derive(Deserialize)]
pub(crate) struct SessionSnapshot {
    pub id: Uuid,
    pub role: String,
    pub interviewee_id: Uuid,
    pub job_title: String,
    pub job_code: String,
    pub state: String,
    pub duration_minutes: i32,
    pub expires_at: String,
    pub deadline_at: Option<String>,
    pub server_time: String,
    pub started_at: Option<String>,
    pub submitted_at: Option<String>,
    pub generation_status: String,
    pub questions_ready: bool,
    pub answer_count: i64,
    pub question_count: i64,
}
#[derive(sqlx::FromRow)]
pub(crate) struct TransitionState {
    pub state: String,
    pub prepared: bool,
    pub active: bool,
}
