use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnnotationRequest {
    pub event_id: Uuid,
    pub decision: String,
    pub note: String,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChatRequest {
    pub question: String,
    pub conversation: Uuid,
    pub page: Option<u32>,
}
#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct LiveQuery {
    pub after: Option<Uuid>,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct SavedResponse {
    pub saved: bool,
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub(crate) struct IntegrityResponse {
    pub risk_score: u64,
    pub weights: Value,
    pub events: Vec<Value>,
    pub summary: String,
    pub notice: String,
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub(crate) struct ExportResponse {
    pub version: i32,
    pub interview: Value,
    pub questions: Vec<Value>,
    pub events: Vec<Value>,
    pub integrity: IntegrityResponse,
    pub integrity_weights: Value,
    pub notice: String,
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub(crate) struct AnalyticsResponse {
    pub groups: Vec<Value>,
    pub notice: String,
}
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
pub(crate) struct MetricsResponse {
    pub assessment_p50_ms: Option<f64>,
    pub assessment_p99_ms: Option<f64>,
    pub generation_p50_ms: Option<f64>,
    pub generation_p99_ms: Option<f64>,
    pub sessions: i64,
    pub observations: i64,
    pub scoring_failures: i64,
    pub observations_per_session: Option<f64>,
}
