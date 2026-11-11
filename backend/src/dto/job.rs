//! Explicit HTTP contracts. Database-only fields never serialize through these DTOs.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize, schemars::JsonSchema)]
pub(crate) struct CreateJobRequest {
    pub job_code: String,
    pub title: String,
    pub description: String,
    pub request_key: Uuid,
    #[serde(default = "default_duration")]
    pub preparation_duration_minutes: i32,
    #[serde(default = "default_set_count")]
    pub question_set_count: i32,
}
fn default_set_count() -> i32 {
    1
}
fn default_duration() -> i32 {
    30
}

#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct JobResponse {
    pub id: Uuid,
    pub job_code: String,
    pub title: String,
    pub description: String,
    pub created_at: String,
    pub preparation_duration_minutes: i32,
    pub question_set_count: i32,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct JobListItemResponse {
    #[serde(flatten)]
    pub job: JobResponse,
    pub session_count: i64,
}
#[derive(Serialize, schemars::JsonSchema)]
pub(crate) struct JobListResponse {
    pub total: i64,
    pub items: Vec<JobListItemResponse>,
    pub next_cursor: Option<Uuid>,
}

#[derive(Deserialize)]
pub(crate) struct JobListQuery {
    pub cursor: Option<Uuid>,
    pub limit: Option<i64>,
    #[serde(default)]
    pub search: String,
    #[serde(default = "all_invitations")]
    pub invitations: String,
    #[serde(default = "newest_first")]
    pub sort: String,
}
fn all_invitations() -> String {
    "all".to_string()
}
fn newest_first() -> String {
    "newest".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Query, http::Uri};

    #[test]
    fn job_query_parses_pagination_and_filters_from_url() {
        let uri: Uri = "/api/v1/jobs?limit=6&search=Rust&invitations=without&sort=oldest"
            .parse()
            .unwrap();
        let Query(query) = Query::<JobListQuery>::try_from_uri(&uri).unwrap();
        assert_eq!(query.limit, Some(6));
        assert_eq!(query.search, "Rust");
        assert_eq!(query.invitations, "without");
        assert_eq!(query.sort, "oldest");
        let plain: Uri = "/api/v1/jobs?limit=6".parse().unwrap();
        let Query(query) = Query::<JobListQuery>::try_from_uri(&plain).unwrap();
        assert_eq!(query.limit, Some(6));
        assert_eq!(query.sort, "newest");
        assert_eq!(query.invitations, "all");
    }
}
