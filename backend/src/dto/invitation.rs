use super::interview::SessionResponse;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct InvitationRequest {
    pub candidate_name: String,
    pub candidate_email: String,
    pub duration_minutes: i32,
    pub request_key: Uuid,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct InvitationResponse {
    pub session: SessionResponse,
    pub invitation_token: Option<String>,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct RecoveredInvitationResponse {
    pub invitation_token: String,
    pub expires_at: String,
    pub replaced_legacy_link: bool,
}
