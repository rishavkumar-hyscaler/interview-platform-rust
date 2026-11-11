//! Internal query models; deliberately do not implement Serialize.
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct Job {
    pub id: Uuid,
    pub interviewer_id: Uuid,
    pub request_key: Uuid,
    pub job_code: String,
    pub title: String,
    pub description: String,
    // PostgreSQL JSON timestamp representation preserves the existing API format.
    pub created_at: String,
    pub preparation_duration_minutes: i32,
    pub question_set_count: i32,
    pub blueprint_preparation: serde_json::Value,
}
#[derive(sqlx::FromRow)]
pub(crate) struct JobListItem {
    #[sqlx(flatten)]
    pub job: Job,
    pub session_count: i64,
}

pub(crate) struct NewJob {
    pub id: Uuid,
    pub interviewer_id: Uuid,
    pub request_key: Uuid,
    pub job_code: String,
    pub title: String,
    pub description: String,
    pub preparation_duration_minutes: i32,
    pub question_set_count: i32,
}

#[derive(Default)]
pub(crate) struct JobFilters {
    pub search: String,
    pub invitations: String,
    pub sort: String,
}
