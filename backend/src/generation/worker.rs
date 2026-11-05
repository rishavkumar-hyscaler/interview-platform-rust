//! Claim, renew, retry and finish generation jobs.
use super::provider::AiProvider;
use crate::infrastructure::interviewer_transaction;
use sqlx::{PgPool, Row};
use tracing::Instrument;
use uuid::Uuid;

use super::pipeline::prepare;
pub(super) const MAX_ATTEMPTS: i32 = 4; // Initial attempt plus three automatic retries.

#[derive(Clone)]
pub(super) struct Claim {
    pub(super) is_pool: bool,
    pub(super) id: Uuid,
    pub(super) recruiter: Uuid,
    pub(super) session: Uuid,
    pub(super) token: Uuid,
    pub(super) seed: Uuid,
    pub(super) attempts: i32,
}
enum Claimed {
    Assigned,
    Generate(Claim),
}
async fn claim(pool: &PgPool) -> anyhow::Result<Option<Claimed>> {
    let interviewers: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer' ORDER BY id")
            .fetch_all(pool)
            .await?;
    // Candidate sessions take priority over stock replenishment.
    for interviewer in &interviewers {
        let mut tx = interviewer_transaction(pool, *interviewer).await?;
        sqlx::query("UPDATE interview_sessions SET generation_status='failed',generation_error='Generation stopped before its final attempt completed',generation_claim_token=NULL,generation_lease_until=NULL WHERE generation_status='running' AND generation_lease_until<now() AND generation_attempts>=$1").bind(MAX_ATTEMPTS).execute(&mut *tx).await?;
        let token = Uuid::new_v4();
        let row=sqlx::query("UPDATE interview_sessions SET generation_status='running',generation_claim_token=$1,generation_lease_until=now()+interval '5 minutes',generation_attempts=generation_attempts+1 WHERE id=(SELECT id FROM interview_sessions WHERE ((generation_status='pending' AND generation_available_at<=now()) OR (generation_status='running' AND generation_lease_until<now())) AND generation_attempts<$2 ORDER BY generation_available_at,id FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING generation_id AS id,id AS session_id,generation_seed AS seed,generation_attempts AS attempts")
            .bind(token).bind(MAX_ATTEMPTS).fetch_optional(&mut *tx).await?;
        if let Some(row) = row {
            let session: Uuid = row.get("session_id");
            if super::bank::assign(&mut tx, *interviewer, session).await? {
                tx.commit().await?;
                return Ok(Some(Claimed::Assigned));
            }
            tx.commit().await?;
            return Ok(Some(Claimed::Generate(Claim {
                is_pool: false,
                id: row.get("id"),
                recruiter: *interviewer,
                session,
                seed: row.get("seed"),
                attempts: row.get("attempts"),
                token,
            })));
        }
        tx.commit().await?;
    }
    claim_stock(pool, &interviewers).await
}

struct StockCandidate {
    owner: Uuid,
    id: Uuid,
    attempts: i32,
    job_created: i64,
    created: i64,
}

async fn claim_stock(pool: &PgPool, interviewers: &[Uuid]) -> anyhow::Result<Option<Claimed>> {
    let mut candidates = Vec::new();
    // Read each owner's queue under RLS. Do not let owner UUID ordering decide
    // which newly created job waits behind an entire historical backlog.
    for owner in interviewers {
        let mut tx = interviewer_transaction(pool, *owner).await?;
        sqlx::query("UPDATE question_sets SET status='failed',last_error='Generation stopped before its final attempt completed',claim_token=NULL,lease_until=NULL WHERE status='running' AND lease_until<now() AND attempts>=$1").bind(MAX_ATTEMPTS).execute(&mut *tx).await?;
        let row = sqlx::query("SELECT p.id,p.attempts,(extract(epoch from j.created_at)*1000000)::bigint AS job_created,(extract(epoch from p.created_at)*1000000)::bigint AS created FROM question_sets p JOIN jobs j ON j.id=p.job_id WHERE p.id IN (SELECT id FROM question_sets WHERE job_id=p.job_id ORDER BY created_at,id LIMIT j.question_set_count) AND ((p.status='pending' AND p.available_at<=now()) OR (p.status='running' AND p.lease_until<now())) AND p.attempts<$1 ORDER BY p.attempts,j.created_at DESC,p.created_at,p.id LIMIT 1")
            .bind(MAX_ATTEMPTS).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        if let Some(row) = row {
            candidates.push(StockCandidate {
                owner: *owner,
                id: row.get("id"),
                attempts: row.get("attempts"),
                job_created: row.get("job_created"),
                created: row.get("created"),
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
        let row = sqlx::query("UPDATE question_sets SET status='running',claim_token=$1,lease_until=now()+interval '5 minutes',attempts=attempts+1 WHERE id=(SELECT id FROM question_sets WHERE id=$2 AND ((status='pending' AND available_at<=now()) OR (status='running' AND lease_until<now())) AND attempts<$3 FOR UPDATE SKIP LOCKED) RETURNING id,seed,attempts")
            .bind(token).bind(candidate.id).bind(MAX_ATTEMPTS).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        if let Some(row) = row {
            let id = row.get("id");
            return Ok(Some(Claimed::Generate(Claim {
                is_pool: true,
                id,
                recruiter: candidate.owner,
                session: id,
                token,
                seed: row.get("seed"),
                attempts: row.get("attempts"),
            })));
        }
    }
    Ok(None)
}
pub async fn process_next(pool: &PgPool, provider: &AiProvider) -> anyhow::Result<bool> {
    let claim = match claim(pool).await? {
        None => return Ok(false),
        Some(Claimed::Assigned) => return Ok(true),
        Some(Claimed::Generate(claim)) => claim,
    };
    let operation = if claim.is_pool {
        "question_set.prepare"
    } else {
        "questions.prepare"
    };
    let mut task = crate::observability::TaskLog::start(operation, claim.session);
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
    tracing::info!(target_id=%claim.session,is_pool=claim.is_pool,attempt=claim.attempts,elapsed_ms=started.elapsed().as_millis() as u64,success=result.is_ok(),"question preparation attempt finished");
    let success = result.is_ok();
    if let Err(failure) = result {
        let message = if failure.downcast_ref::<sqlx::Error>().is_some() {
            "Generation database operation failed".to_string()
        } else {
            failure.to_string().chars().take(300).collect()
        };
        tracing::warn!(job_id=%claim.id,attempt=claim.attempts,"question generation failed");
        let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
        if claim.is_pool {
            sqlx::query("UPDATE question_sets SET status=CASE WHEN attempts>=$4 THEN 'failed' ELSE 'pending' END,last_error=$3,available_at=now()+attempts*interval '5 seconds',claim_token=NULL,lease_until=NULL WHERE id=$1 AND claim_token=$2 AND status='running'")
                .bind(claim.id).bind(claim.token).bind(message).bind(MAX_ATTEMPTS).execute(&mut *tx).await?;
        } else {
            sqlx::query("UPDATE interview_sessions SET generation_status=CASE WHEN generation_attempts>=$4 THEN 'failed' ELSE 'pending' END,generation_error=$3,generation_available_at=now()+generation_attempts*interval '5 seconds',generation_claim_token=NULL,generation_lease_until=NULL WHERE id=$1 AND generation_claim_token=$2 AND generation_status='running'")
            .bind(claim.session).bind(claim.token).bind(message).bind(MAX_ATTEMPTS).execute(&mut *tx).await?;
        }
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
        let changed = if claim.is_pool {
            sqlx::query("UPDATE question_sets SET lease_until=now()+interval '5 minutes' WHERE id=$1 AND claim_token=$2 AND status='running' AND lease_until>now()")
                .bind(claim.id).bind(claim.token).execute(&mut *tx).await?.rows_affected()
        } else {
            sqlx::query("UPDATE interview_sessions SET generation_lease_until=now()+interval '5 minutes' WHERE id=$1 AND generation_claim_token=$2 AND generation_status='running' AND generation_lease_until>now() AND state IN ('invited','ready') AND expires_at>now()").bind(claim.session).bind(claim.token).execute(&mut *tx).await?.rows_affected()
        };
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
