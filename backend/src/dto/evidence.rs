use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub(crate) struct EvidenceResponse {
    pub asset_id: Uuid,
    pub event_id: Uuid,
    pub mime: String,
    pub bytes: u64,
    pub sha256: String,
    pub parent_signature: Option<String>,
    pub source: String,
    pub evidence_source: String,
    pub capture_surface: String,
    pub captured_at_ms: Option<i64>,
    pub verified: bool,
    pub capture: String,
    pub notice: String,
    pub signature: String,
}
