//! Claim, renew, retry and finish generation jobs.
use super::provider;
use super::provider::AiProvider;
use crate::infrastructure::interviewer_transaction;
use sqlx::{PgPool, Row};
use tracing::Instrument;
use uuid::Uuid;

use super::pipeline::prepare;
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
        sqlx::query("UPDATE interview_sessions SET generation_status='failed',generation_error='Generation stopped before its final attempt completed',generation_claim_token=NULL,generation_lease_until=NULL WHERE generation_status='running' AND generation_lease_until<now() AND generation_attempts>=3").execute(&mut *tx).await?;
        let token = Uuid::new_v4();
        let row=sqlx::query("UPDATE interview_sessions SET generation_status='running',generation_claim_token=$1,generation_lease_until=now()+interval '5 minutes',generation_attempts=generation_attempts+1 WHERE id=(SELECT id FROM interview_sessions WHERE ((generation_status='pending' AND generation_available_at<=now()) OR (generation_status='running' AND generation_lease_until<now())) AND generation_attempts<3 ORDER BY generation_available_at,id FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING generation_id AS id,id AS session_id,generation_seed AS seed,generation_attempts AS attempts")
            .bind(token).fetch_optional(&mut *tx).await?;
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
        sqlx::query("UPDATE question_sets SET status='failed',last_error='Generation stopped before its final attempt completed',claim_token=NULL,lease_until=NULL WHERE status='running' AND lease_until<now() AND attempts>=3").execute(&mut *tx).await?;
        let row = sqlx::query("SELECT p.id,p.attempts,(extract(epoch from j.created_at)*1000000)::bigint AS job_created,(extract(epoch from p.created_at)*1000000)::bigint AS created FROM question_sets p JOIN jobs j ON j.id=p.job_id WHERE p.assigned_session_id IS NULL AND ((p.status='pending' AND p.available_at<=now()) OR (p.status='running' AND p.lease_until<now())) AND p.attempts<3 ORDER BY p.attempts,j.created_at DESC,p.created_at,p.id LIMIT 1")
            .fetch_optional(&mut *tx).await?;
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
        let row = sqlx::query("UPDATE question_sets SET status='running',claim_token=$1,lease_until=now()+interval '5 minutes',attempts=attempts+1 WHERE id=(SELECT id FROM question_sets WHERE id=$2 AND assigned_session_id IS NULL AND ((status='pending' AND available_at<=now()) OR (status='running' AND lease_until<now())) AND attempts<3 FOR UPDATE SKIP LOCKED) RETURNING id,seed,attempts")
            .bind(token).bind(candidate.id).fetch_optional(&mut *tx).await?;
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
    let result = tokio::select! {result=prepare(pool,provider,&claim).instrument(task.span())=>result.map_err(super::bounded_failure),result=renew_lease(pool,&claim)=>result};
    tracing::info!(target_id=%claim.session,is_pool=claim.is_pool,attempt=claim.attempts,elapsed_ms=started.elapsed().as_millis() as u64,success=result.is_ok(),"question preparation attempt finished");
    let success = result.is_ok();
    if let Err(failure) = result {
        let fatal = failure
            .downcast_ref::<provider::ProviderFailure>()
            .is_some_and(|error| !error.retryable)
            || failure
                .downcast_ref::<super::PreparationRejected>()
                .is_some();
        let message = if failure.downcast_ref::<sqlx::Error>().is_some() {
            "Generation database operation failed".to_string()
        } else {
            failure.to_string().chars().take(300).collect()
        };
        tracing::warn!(job_id=%claim.id,attempt=claim.attempts,"question generation failed");
        let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
        if claim.is_pool {
            sqlx::query("UPDATE question_sets SET status=CASE WHEN attempts>=3 OR $4 THEN 'failed' ELSE 'pending' END,last_error=$3,available_at=now()+attempts*interval '5 seconds',claim_token=NULL,lease_until=NULL WHERE id=$1 AND claim_token=$2 AND status='running'")
                .bind(claim.id).bind(claim.token).bind(message).bind(fatal).execute(&mut *tx).await?;
        } else {
            sqlx::query("UPDATE interview_sessions SET generation_status=CASE WHEN generation_attempts>=3 OR $4 THEN 'failed' ELSE 'pending' END,generation_error=$3,generation_available_at=now()+generation_attempts*interval '5 seconds',generation_claim_token=NULL,generation_lease_until=NULL WHERE id=$1 AND generation_claim_token=$2 AND generation_status='running'")
            .bind(claim.session).bind(claim.token).bind(message).bind(fatal).execute(&mut *tx).await?;
        }
        tx.commit().await?;
    }
    task.finish(success);
    Ok(true)
}
async fn renew_lease(pool: &PgPool, claim: &Claim) -> anyhow::Result<()> {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
        let changed = if claim.is_pool {
            sqlx::query("UPDATE question_sets SET lease_until=now()+interval '5 minutes' WHERE id=$1 AND claim_token=$2 AND status='running' AND lease_until>now() AND assigned_session_id IS NULL")
                .bind(claim.id).bind(claim.token).execute(&mut *tx).await?.rows_affected()
        } else {
            sqlx::query("UPDATE interview_sessions SET generation_lease_until=now()+interval '5 minutes' WHERE id=$1 AND generation_claim_token=$2 AND generation_status='running' AND generation_lease_until>now() AND state IN ('invited','ready') AND expires_at>now()").bind(claim.session).bind(claim.token).execute(&mut *tx).await?.rows_affected()
        };
        tx.commit().await?;
        anyhow::ensure!(changed == 1, "Generation job lease was lost");
    }
}
