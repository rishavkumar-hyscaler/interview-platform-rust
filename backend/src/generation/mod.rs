//! Question generation: HTTP scheduling, leased workers and validated publication pipeline.
pub mod provider;
mod quality;
pub mod types;
use serde_json::json;
use uuid::Uuid;
const PROMPT_VERSION: &str = "jd-questions-v2";
#[derive(Clone, Copy)]
pub(crate) struct AutomaticPreparation(pub bool);

pub(crate) async fn prepare_automatically(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner: Uuid,
    session: Uuid,
    enabled: bool,
) -> std::result::Result<bool, sqlx::Error> {
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

mod http;
mod pipeline;
mod worker;
pub use http::routes;
pub use worker::process_next;
