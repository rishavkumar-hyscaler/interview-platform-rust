//! Claim, renew, retry and finish generation jobs.
use super::provider::AiProvider;
use crate::infrastructure::interviewer_transaction;
use crate::repositories::generation_workflow_repository as repo;
use sqlx::PgPool;
use tracing::Instrument;
use uuid::Uuid;

use super::pipeline::prepare;
pub(crate) const MAX_ATTEMPTS: i32 = 4; // Initial attempt plus three automatic retries.

#[derive(Clone)]
pub(super) struct Claim {
    pub(super) id: Uuid,
    pub(super) recruiter: Uuid,
    pub(super) token: Uuid,
    pub(super) seed: Uuid,
    pub(super) attempts: i32,
}
async fn claim(pool: &PgPool) -> anyhow::Result<Option<Claim>> {
    let interviewers: Vec<Uuid> = repo::interviewers(pool).await?;
    claim_stock(pool, &interviewers).await
}

struct StockCandidate {
    owner: Uuid,
    id: Uuid,
    attempts: i32,
    job_created: i64,
    created: i64,
}

async fn claim_stock(pool: &PgPool, interviewers: &[Uuid]) -> anyhow::Result<Option<Claim>> {
    let mut candidates = Vec::new();
    // Read each owner's queue under RLS. Do not let owner UUID ordering decide
    // which newly created job waits behind an entire historical backlog.
    for owner in interviewers {
        let mut tx = interviewer_transaction(pool, *owner).await?;
        repo::expire_final_attempts(&mut tx, MAX_ATTEMPTS).await?;
        let row = repo::queue_candidate(&mut tx, MAX_ATTEMPTS).await?;
        tx.commit().await?;
        if let Some(row) = row {
            candidates.push(StockCandidate {
                owner: *owner,
                id: row.id,
                attempts: row.attempts,
                job_created: row.job_created,
                created: row.created,
            });
        }
    }
    candidates.sort_by(|a, b| {
        a.attempts
            .cmp(&b.attempts)
            .then(b.job_created.cmp(&a.job_created))
            .then(a.created.cmp(&b.created))
            .then(a.id.cmp(&b.id))
    });
    for candidate in candidates {
        let mut tx = interviewer_transaction(pool, candidate.owner).await?;
        let token = Uuid::new_v4();
        // Recheck eligibility after selection; another process may have claimed it.
        let row = repo::claim_set(&mut tx, token, candidate.id, MAX_ATTEMPTS).await?;
        tx.commit().await?;
        if let Some(row) = row {
            let id = row.id;
            return Ok(Some(Claim {
                id,
                recruiter: candidate.owner,
                token,
                seed: row.seed,
                attempts: row.attempts,
            }));
        }
    }
    Ok(None)
}
pub async fn process_next(pool: &PgPool, provider: &AiProvider) -> anyhow::Result<bool> {
    let claim = match claim(pool).await? {
        None => return Ok(false),
        Some(claim) => claim,
    };
    let mut task = crate::observability::TaskLog::start("question_set.prepare", claim.id);
    let started = std::time::Instant::now();
    let work_pool = pool.clone();
    let work_provider = provider.clone();
    let work_claim = claim.clone();
    let mut work = PreparationTask(tokio::spawn(
        async move { prepare(&work_pool, &work_provider, &work_claim).await }
            .instrument(task.span()),
    ));
    let result = tokio::select! {
        result = &mut work.0 => preparation_result(result),
        result = renew_lease(pool, &claim) => {
            work.0.abort();
            let _ = (&mut work.0).await;
            result
        }
    };
    tracing::info!(target_id=%claim.id,attempt=claim.attempts,elapsed_ms=started.elapsed().as_millis() as u64,success=result.is_ok(),"question preparation attempt finished");
    let success = result.is_ok();
    if let Err(failure) = result {
        let message = if failure.downcast_ref::<sqlx::Error>().is_some() {
            "Generation database operation failed".to_string()
        } else {
            failure.to_string().chars().take(300).collect()
        };
        tracing::warn!(job_id=%claim.id,attempt=claim.attempts,"question generation failed");
        let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
        repo::fail_attempt(&mut tx, claim.id, claim.token, message, MAX_ATTEMPTS).await?;
        tx.commit().await?;
    }
    task.finish(success);
    Ok(true)
}
// Dropping process_next during shutdown must cancel its in-flight preparation too.
struct PreparationTask(tokio::task::JoinHandle<anyhow::Result<()>>);
impl Drop for PreparationTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
fn preparation_result(
    result: Result<anyhow::Result<()>, tokio::task::JoinError>,
) -> anyhow::Result<()> {
    match result {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!(
            "Question preparation stopped unexpectedly; saved questions are preserved"
        )),
    }
}

async fn renew_lease(pool: &PgPool, claim: &Claim) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
        let changed = repo::renew_lease(&mut tx, claim.id, claim.token)
            .await?
            .rows_affected();
        tx.commit().await?;
        anyhow::ensure!(changed == 1, "Generation job lease was lost");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn unexpected_preparation_panic_becomes_a_retryable_error() {
        let task = tokio::spawn(async {
            panic!("synthetic panic payload");
            #[allow(unreachable_code)]
            Ok(())
        });
        let error = preparation_result(task.await).unwrap_err();
        assert!(error.to_string().contains("stopped unexpectedly"));
        assert!(!error.to_string().contains("synthetic panic payload"));
        assert_eq!(MAX_ATTEMPTS, 4);
    }
}
