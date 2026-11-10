//! Internal session query models. No Serialize implementations.
use uuid::Uuid;
#[derive(sqlx::FromRow)]
pub(crate) struct Session {
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
#[derive(sqlx::FromRow)]
pub(crate) struct SessionListItem {
    #[sqlx(flatten)]
    pub session: Session,
    pub job_title: String,
    pub job_code: String,
}
#[derive(sqlx::FromRow)]
pub(crate) struct SessionDetail {
    #[sqlx(flatten)]
    pub item: SessionListItem,
    pub description: String,
    pub question_count: i64,
    pub answer_count: i64,
    pub invitation_recoverable: bool,
    pub invitation_active: bool,
}
#[derive(sqlx::FromRow)]
pub(crate) struct Summary {
    pub jobs: i64,
    pub sessions: i64,
    pub active: i64,
    pub reviewable: i64,
    pub daily_submissions: Vec<i64>,
    pub daily_roles: Vec<i64>,
    pub daily_invitations: Vec<i64>,
    pub daily_starts: Vec<i64>,
    pub activity_end_date: String,
}
