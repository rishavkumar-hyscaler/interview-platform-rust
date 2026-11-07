use serde_json::Value;
use uuid::Uuid;
#[derive(sqlx::FromRow)]
pub(crate) struct AuditEvent {
    pub id: Uuid,
    pub interviewer_id: Uuid,
    pub actor_id: Option<Uuid>,
    pub entity_id: Option<Uuid>,
    pub action: String,
    pub metadata: Value,
    pub created_at: String,
}
