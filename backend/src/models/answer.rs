use crate::models::candidate::SessionSnapshot;
use serde::Deserialize;
use uuid::Uuid;
#[derive(Deserialize)]
pub(crate) struct Question {
    pub id: Uuid,
    pub position: i32,
    pub prompt: String,
    pub kind: String,
    pub difficulty: String,
    pub answer: String,
    pub revision: i32,
    pub saved_at: Option<String>,
}
#[derive(sqlx::FromRow)]
pub(crate) struct CurrentAnswer {
    pub content: String,
    pub revision: i32,
}
#[derive(Deserialize)]
pub(crate) struct SavedAnswer {
    pub question_id: Uuid,
    pub revision: i32,
    pub saved_at: String,
    pub answer_count: i64,
}
pub(crate) struct Room {
    pub session: SessionSnapshot,
    pub question: Option<Question>,
    pub question_started_at: Option<String>,
}
