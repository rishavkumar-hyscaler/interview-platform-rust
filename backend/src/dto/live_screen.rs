use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Deserialize)]
pub(crate) struct Signal {
    pub action: String,
    pub id: Option<Uuid>,
    pub sdp: Option<String>,
}
#[derive(Serialize)]
pub(crate) struct View {
    pub id: Uuid,
    pub offer: Option<String>,
    pub answer: Option<String>,
    pub ice_servers: serde_json::Value,
}
