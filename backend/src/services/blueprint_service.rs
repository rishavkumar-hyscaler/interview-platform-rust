//! Blueprint preparation is durable background work, separate from HTTP creation.
use crate::{
    audit::record,
    generation::{PROMPT_VERSION, provider::AiProvider},
    infrastructure::interviewer_transaction,
    repositories::{
        blueprint_repository as repo, generation_workflow_repository as generation_repo,
    },
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub(crate) async fn process_next(pool: &PgPool, provider: &AiProvider) -> anyhow::Result<bool> {
    for owner in generation_repo::interviewers(pool).await? {
        let mut tx = interviewer_transaction(pool, owner).await?;
        let Some(job) = repo::claimable(&mut tx).await? else {
            tx.commit().await?;
            continue;
        };
        // The row is locked now. Re-read audit state with a fresh snapshot so
        // another worker's just-committed claim cannot be claimed again.
        if repo::state(&mut tx, job.id).await? != job.preparation {
            tx.commit().await?;
            continue;
        }
        let previous_attempts = job
            .preparation
            .as_ref()
            .and_then(|p| p["attempts"].as_i64())
            .unwrap_or(0);
        if previous_attempts >= 4 {
            record(&mut tx, owner, None, job.id, "job.blueprint_task",
                json!({"status":"failed","attempts":4,"last_error":"Blueprint worker stopped during its final attempt. Retry preparation."}))
                .await.map_err(|e| anyhow::anyhow!(e.1))?;
            tx.commit().await?;
            return Ok(true);
        }
        let attempts = previous_attempts + 1;
        let claim = Uuid::new_v4();
        let lease_until = repo::deadline(&mut tx, 900).await?;
        record(
            &mut tx,
            owner,
            None,
            job.id,
            "job.blueprint_task",
            json!({
                "status":"running","attempts":attempts,"claim":claim,"lease_until":lease_until
            }),
        )
        .await
        .map_err(|e| anyhow::anyhow!(e.1))?;
        tx.commit().await?;

        let result =
            crate::generation::blueprint_for_job(provider, &job.description, job.duration).await;
        let mut tx = interviewer_transaction(pool, owner).await?;
        let Some(ready) = repo::lock(&mut tx, job.id).await? else {
            return Ok(true);
        };
        let state = repo::state(&mut tx, job.id).await?;
        let expected_claim = claim.to_string();
        if ready
            || state
                .as_ref()
                .and_then(|p| p["claim"].as_str())
                != Some(expected_claim.as_str())
        {
            return Ok(true);
        }
        let metadata = match result {
            Ok(blueprint) => {
                generation_repo::save_blueprint(
                    &mut tx,
                    job.id,
                    blueprint,
                    &provider.model,
                    PROMPT_VERSION,
                )
                .await?;
                json!({"status":"completed","attempts":attempts})
            }
            Err(_) => {
                let available_at = repo::deadline(&mut tx, 15).await?;
                tracing::warn!(job_id=%job.id, attempt=attempts, "Job blueprint preparation failed");
                json!({"status":if attempts>=4 {"failed"} else {"pending"},"attempts":attempts,
                    "available_at":available_at,"last_error":"Blueprint preparation failed. Saved job details are preserved."})
            }
        };
        record(&mut tx, owner, None, job.id, "job.blueprint_task", metadata)
            .await
            .map_err(|e| anyhow::anyhow!(e.1))?;
        tx.commit().await?;
        return Ok(true);
    }
    Ok(false)
}

pub(crate) async fn retry(pool: &PgPool, owner: Uuid, job: Uuid) -> crate::Result<()> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let ready = repo::lock(&mut tx, job)
        .await?
        .ok_or_else(crate::error::missing)?;
    let state = repo::state(&mut tx, job).await?;
    if !ready
        && (state.is_none()
            || state
                .as_ref()
                .is_some_and(|p| p["status"] == "failed"))
    {
        record(
            &mut tx,
            owner,
            Some(owner),
            job,
            "job.blueprint_task",
            json!({"status":"pending","attempts":0}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
