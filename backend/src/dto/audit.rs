use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;
#[derive(Serialize)]
pub(crate) struct AuditEventResponse {
    pub id: Uuid,
    pub interviewer_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
    pub actor_email: Option<String>,
    pub candidate_name: Option<String>,
    pub candidate_email: Option<String>,
    pub job_title: Option<String>,
    pub job_code: Option<String>,
    pub question_position: Option<i32>,
    pub action: String,
    pub metadata: Value,
    pub created_at: String,
}
#[derive(Serialize)]
pub(crate) struct AuditPageResponse {
    pub items: Vec<AuditEventResponse>,
    pub next_cursor: Option<Uuid>,
}
