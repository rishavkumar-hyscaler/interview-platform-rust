//! Internal generation status query rows, never HTTP response types.
use serde_json::Value;
use uuid::Uuid;
#[derive(sqlx::FromRow)]
pub(crate) struct SessionGeneration {
    pub status: String,
    pub job: Option<Value>,
    pub blueprint: Option<Value>,
}
#[derive(sqlx::FromRow)]
pub(crate) struct JobPreparation {
    pub blueprint: Option<Value>,
    pub preparation_duration_minutes: i32,
    pub question_set_count: i32,
}
pub(crate) struct SessionStatus {
    pub row: SessionGeneration,
    pub questions: Vec<Value>,
}
pub(crate) struct SetStatus {
    pub row: JobPreparation,
    pub sets: Vec<Value>,
    pub questions: Vec<Value>,
}

#[derive(sqlx::FromRow)]
pub(crate) struct AssignmentCandidate {
    pub job_id: Uuid,
    pub duration_minutes: i32,
}
#[derive(sqlx::FromRow)]
pub(crate) struct QueueCandidate {
    pub id: Uuid,
    pub attempts: i32,
    pub job_created: i64,
    pub created: i64,
}
#[derive(sqlx::FromRow)]
pub(crate) struct ClaimedSet {
    pub id: Uuid,
    pub seed: Uuid,
    pub attempts: i32,
}
#[derive(sqlx::FromRow)]
pub(crate) struct PreparationJob {
    pub job_id: Uuid,
    pub description: String,
    pub duration_minutes: i32,
    pub generation_error: Option<String>,
}
#[derive(sqlx::FromRow)]
pub(crate) struct SavedQuestionTime {
    pub position: i32,
    pub minutes: i32,
}

/// Database insert values; separate from API input and AI generation schemas.
pub(crate) struct PreparedQuestionInsert<'a> {
    pub id: Uuid,
    pub owner: Uuid,
    pub job: Uuid,
    pub position: i32,
    pub kind: &'a str,
    pub difficulty: &'a str,
    pub prompt: &'a str,
    pub rubric: Value,
    pub competency: &'a str,
    pub fingerprint: String,
    pub vector: String,
    pub model: &'a str,
    pub dimensions: i32,
    pub set: Uuid,
}
