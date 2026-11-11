//! Question generation: HTTP scheduling, leased workers and validated publication pipeline.
pub mod provider;
mod quality;
pub mod types;
pub(crate) const PROMPT_VERSION: &str = "jd-questions-v6";

pub(crate) mod bank;
mod checkpoint;
mod diagnostics;
mod drafts;
mod pipeline;
mod plan;
pub(crate) mod worker;
pub use crate::controllers::generation_controller::routes;
pub async fn process_next(
    pool: &sqlx::PgPool,
    provider: &provider::AiProvider,
) -> anyhow::Result<bool> {
    if crate::services::blueprint_service::process_next(pool, provider).await? {
        return Ok(true);
    }
    worker::process_next(pool, provider).await
}

/// Preserve bounded diagnostic messages; the worker retries every failure category.
#[derive(Debug)]
pub(super) struct PreparationRejected(pub String);
impl std::fmt::Display for PreparationRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PreparationRejected {}
pub(super) fn bounded_failure(error: anyhow::Error) -> anyhow::Error {
    if error
        .downcast_ref::<provider::ProviderFailure>()
        .is_some()
        || error
            .downcast_ref::<sqlx::Error>()
            .is_some()
        || error
            .downcast_ref::<provider::TransportFailure>()
            .is_some()
    {
        error
    } else {
        PreparationRejected(error.to_string()).into()
    }
}

/// Prepare the job's fixed blueprint before any candidate is invited.
pub(crate) async fn blueprint_for_job(
    provider: &provider::AiProvider,
    description: &str,
    duration: i32,
) -> anyhow::Result<serde_json::Value> {
    let blueprint = quality::blueprint(provider, description, duration, "").await?;
    serde_json::to_value(blueprint).map_err(Into::into)
}
