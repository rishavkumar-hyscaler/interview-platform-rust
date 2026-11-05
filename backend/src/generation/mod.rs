//! Question generation: HTTP scheduling, leased workers and validated publication pipeline.
pub mod provider;
mod quality;
pub mod types;
use serde_json::json;
use uuid::Uuid;
const PROMPT_VERSION: &str = "jd-questions-v4";
#[derive(Clone, Copy)]
pub(crate) struct AutomaticPreparation(pub bool);

pub(crate) async fn prepare_automatically(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
    session: Uuid,
    enabled: bool,
) -> std::result::Result<bool, sqlx::Error> {
    if bank::assign(tx, owner, session).await? {
        return Ok(true);
    }
    if !enabled {
        return Ok(false);
    }
    let changed = sqlx::query("UPDATE interview_sessions s SET generation_id=coalesce(generation_id,$2),generation_seed=coalesce(generation_seed,$3),generation_status='pending',generation_available_at=now(),generation_error=NULL WHERE s.id=$1 AND s.generation_status='not_started' AND s.state IN ('invited','ready') AND s.expires_at>clock_timestamp() AND NOT EXISTS(SELECT 1 FROM interview_questions q WHERE q.session_id=s.id)")
        .bind(session).bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(&mut **tx).await?.rows_affected();
    if changed > 0 {
        sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'questions.queued',$4)")
            .bind(Uuid::new_v4()).bind(owner).bind(session).bind(json!({"actor":"system","source":"automatic_preparation","prompt_version":PROMPT_VERSION})).execute(&mut **tx).await?;
    }
    Ok(changed > 0)
}

pub(crate) mod bank;
mod bank_http;
mod checkpoint;
mod diagnostics;
mod drafts;
mod http;
mod pipeline;
mod references;
mod worker;
pub use http::routes;
pub use worker::process_next;

/// Quality failures are already repaired locally. Do not regenerate whole sets
/// three more times; retry only transient provider/database failures automatically.
#[derive(Debug)]
pub(super) struct PreparationRejected(pub String);
impl std::fmt::Display for PreparationRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PreparationRejected {}
pub(super) fn bounded_failure(error: anyhow::Error) -> anyhow::Error {
    if error.downcast_ref::<provider::ProviderFailure>().is_some()
        || error.downcast_ref::<sqlx::Error>().is_some()
        || error.downcast_ref::<provider::TransportFailure>().is_some()
    {
        error
    } else {
        PreparationRejected(error.to_string()).into()
    }
}
