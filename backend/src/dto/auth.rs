use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Deserialize)]
pub(crate) struct LoginRequest {
    pub email: String,
    pub password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegisterRequest {
    pub email: String,
    pub password: String,
}
#[derive(Serialize)]
pub(crate) struct SessionResponse {
    pub token: String,
    pub user_id: Uuid,
    pub role: &'static str,
}
#[derive(Serialize)]
pub(crate) struct UserResponse {
    pub email: String,
    pub user_id: Uuid,
    pub role: &'static str,
}
