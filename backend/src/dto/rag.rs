use serde::Serialize;
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct IndexStatusResponse {
    pub status: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub configured: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_revision: Option<i64>,
    // When a job exists, these fields remain present even if their values are null.
    #[serde(flatten)]
    pub details: Option<IndexJobDetails>,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct IndexJobDetails {
    pub last_error: Option<String>,
    pub model: Option<String>,
    pub dimensions: Option<i32>,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct RetryResponse {
    pub status: &'static str,
}
