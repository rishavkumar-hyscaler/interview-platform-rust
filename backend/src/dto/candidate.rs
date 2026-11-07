use serde::Serialize;
use uuid::Uuid;
#[derive(Serialize)]
pub(crate) struct CandidateSessionResponse {
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
