//! Credential rows never implement Serialize.
use uuid::Uuid;
#[derive(sqlx::FromRow)]
pub(crate) struct Credentials {
    pub id: Uuid,
    pub password_hash: String,
}
pub(crate) struct Session {
    pub token: String,
    pub user_id: Uuid,
}
