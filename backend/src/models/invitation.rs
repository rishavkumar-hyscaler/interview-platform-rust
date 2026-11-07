use crate::models::interview::Session;
#[derive(sqlx::FromRow)]
pub(crate) struct InvitationState {
    pub state: String,
    pub active: bool,
}
#[derive(sqlx::FromRow)]
pub(crate) struct InvitationToken {
    pub token_hash: String,
    pub sealed_token: Option<String>,
    pub expires_at: String,
}
pub(crate) struct CreatedInvitation {
    pub session: Session,
    pub token: Option<String>,
    pub created: bool,
}
pub(crate) struct RecoveredInvitation {
    pub token: String,
    pub expires_at: String,
    pub replaced_legacy_link: bool,
}
