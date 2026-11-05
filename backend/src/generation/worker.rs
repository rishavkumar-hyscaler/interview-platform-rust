//! Claim, renew, retry and finish generation jobs.
use super::provider;
use super::provider::AiProvider;
use crate::infrastructure::interviewer_transaction;
use sqlx::{PgPool, Row};
use tracing::Instrument;
use uuid::Uuid;

use super::pipeline::prepare;
pub(super) struct Claim {
    pub(super) id: Uuid,
    pub(super) recruiter: Uuid,
    pub(super) session: Uuid,
    pub(super) token: Uuid,
    pub(super) seed: Uuid,
    pub(super) attempts: i32,
}
async fn claim(pool: &PgPool) -> anyhow::Result<Option<Claim>> {
    let interviewers: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer' ORDER BY id")
            .fetch_all(pool)
            .await?;
    for interviewer in interviewers {
        let mut tx = interviewer_transaction(pool, interviewer).await?;
        sqlx::query("UPDATE interview_sessions SET generation_status='failed',generation_error='Generation stopped before its final attempt completed',generation_claim_token=NULL,generation_lease_until=NULL WHERE generation_status='running' AND generation_lease_until<now() AND generation_attempts>=3").execute(&mut *tx).await?;
        let token = Uuid::new_v4();
        let row=sqlx::query("UPDATE interview_sessions SET generation_status='running',generation_claim_token=$1,generation_lease_until=now()+interval '5 minutes',generation_attempts=generation_attempts+1 WHERE id=(SELECT id FROM interview_sessions WHERE ((generation_status='pending' AND generation_available_at<=now()) OR (generation_status='running' AND generation_lease_until<now())) AND generation_attempts<3 ORDER BY generation_available_at,id FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING generation_id AS id,id AS session_id,generation_seed AS seed,generation_attempts AS attempts")
            .bind(token).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        if let Some(r) = row {
            return Ok(Some(Claim {
                id: r.get("id"),
                recruiter: interviewer,
                session: r.get("session_id"),
                seed: r.get("seed"),
                attempts: r.get("attempts"),
                token,
            }));
        }
    }
    Ok(None)
}
pub async fn process_next(pool: &PgPool, provider: &AiProvider) -> anyhow::Result<bool> {
    let Some(claim) = claim(pool).await? else {
        return Ok(false);
    };
    let mut task = crate::observability::TaskLog::start("questions.prepare", claim.session);
    let started = std::time::Instant::now();
    let result = tokio::select! {result=prepare(pool,provider,&claim).instrument(task.span())=>result,result=renew_lease(pool,&claim)=>result};
    tracing::info!(session_id=%claim.session,attempt=claim.attempts,elapsed_ms=started.elapsed().as_millis() as u64,success=result.is_ok(),"question preparation attempt finished");
    let success = result.is_ok();
    if let Err(failure) = result {
        let fatal = failure
            .downcast_ref::<provider::ProviderFailure>()
            .is_some_and(|error| !error.retryable);
        let message = if failure.downcast_ref::<sqlx::Error>().is_some() {
            "Generation database operation failed".to_string()
        } else {
            failure.to_string().chars().take(300).collect()
        };
        tracing::warn!(job_id=%claim.id,attempt=claim.attempts,"question generation failed");
        let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
        sqlx::query("UPDATE interview_sessions SET generation_status=CASE WHEN generation_attempts>=3 OR $4 THEN 'failed' ELSE 'pending' END,generation_error=$3,generation_available_at=now()+generation_attempts*interval '5 seconds',generation_claim_token=NULL,generation_lease_until=NULL WHERE id=$1 AND generation_claim_token=$2 AND generation_status='running'")
            .bind(claim.session).bind(claim.token).bind(message).bind(fatal).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    task.finish(success);
    Ok(true)
}
async fn renew_lease(pool: &PgPool, claim: &Claim) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
        let changed=sqlx::query("UPDATE interview_sessions SET generation_lease_until=now()+interval '5 minutes' WHERE id=$1 AND generation_claim_token=$2 AND generation_status='running' AND generation_lease_until>now() AND state IN ('invited','ready') AND expires_at>now()").bind(claim.session).bind(claim.token).execute(&mut *tx).await?.rows_affected();
        tx.commit().await?;
        anyhow::ensure!(changed == 1, "Generation job lease was lost");
    }
}
