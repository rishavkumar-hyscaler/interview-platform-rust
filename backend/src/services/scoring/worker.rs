//! Claim and score submitted sessions, then commit validated results.
use crate::repositories::scoring_task_repository as repo;
use crate::{
    audit::record as audit, generation::provider::AiProvider,
    infrastructure::interviewer_transaction,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tracing::Instrument;
use uuid::Uuid;

use super::rubric::{competency_profile, evaluate};
pub async fn process_next(pool: &PgPool, provider: &AiProvider) -> anyhow::Result<bool> {
    let owners: Vec<Uuid> = repo::interviewers(pool).await?;
    for owner in owners {
        let mut tx = interviewer_transaction(pool, owner).await?;
        let exhausted: Option<Uuid> = repo::exhausted_session(&mut tx).await?;
        if let Some(id) = exhausted {
            repo::mark_submitted(&mut tx, id).await?;
            audit(
                &mut tx,
                owner,
                None,
                id,
                "assessment.exhausted",
                json!({"reason":"stale_final_attempt"}),
            )
            .await
            .map_err(|_| anyhow::anyhow!("Assessment recovery audit failed"))?;
            tx.commit().await?;
            return Ok(true);
        }

        let session: Option<Uuid> = repo::next_session(&mut tx).await?;
        let Some(session) = session else {
            tx.commit().await?;
            continue;
        };
        let claim = Uuid::new_v4();
        audit(
            &mut tx,
            owner,
            None,
            session,
            "assessment.started",
            json!({"claim":claim,"model":provider.model}),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Assessment audit failed"))?;
        repo::mark_scoring(&mut tx, session).await?;
        let questions: Vec<Value> = repo::questions(&mut tx, session).await?;
        tx.commit().await?;
        let mut task = crate::observability::TaskLog::start("assessment.score", session);
        let started = std::time::Instant::now();
        let result = assess(provider, &questions, pool, owner, session, claim)
            .instrument(task.span())
            .await;
        tracing::info!(%session,elapsed_ms=started.elapsed().as_millis() as u64,success=result.is_ok(),"assessment attempt finished");
        let mut tx = interviewer_transaction(pool, owner).await?;
        repo::lock_session(&mut tx, session).await?;
        let current: Value = repo::current_claim(&mut tx, session).await?;
        if current["claim"] != json!(claim) {
            tx.commit().await?;
            return Ok(true);
        }
        let success = result.is_ok();
        match result {
            Ok(scores) => {
                let blueprint: Option<Value> = repo::blueprint(&mut tx, session).await?;
                let profile = competency_profile(&scores, blueprint.as_ref());

                audit(&mut tx,owner,None,session,"assessment.completed",json!({"questions":scores,"competency_profile":profile,"model":provider.model,"version":"rubric-v1","human_review_required":true,"claim":claim})).await.map_err(|_| anyhow::anyhow!("Assessment audit failed"))?;
                repo::mark_completed(&mut tx, session).await?;
            }
            Err(error) => {
                let failure = error.downcast_ref::<super::recovery::QuestionFailure>();
                let reason = failure
                    .map(|f| f.reason.clone())
                    .unwrap_or_else(|| "Scoring persistence or claim validation failed".into());
                let position = failure.map(|f| f.position);
                tracing::warn!(%session,?position,%reason,"assessment attempt failed");
                audit(
                    &mut tx,
                    owner,
                    None,
                    session,
                    "assessment.failed",
                    json!({"claim":claim,"reason":reason,"question_position":position}),
                )
                .await
                .map_err(|_| anyhow::anyhow!("Assessment audit failed"))?;
                repo::mark_submitted(&mut tx, session).await?;
            }
        }
        tx.commit().await?;
        task.finish(success);
        return Ok(true);
    }
    Ok(false)
}
async fn assess(
    provider: &AiProvider,
    questions: &[Value],
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
    claim: Uuid,
) -> anyhow::Result<Vec<Value>> {
    anyhow::ensure!(!questions.is_empty(), "No questions");
    let mut scores = Vec::new();
    for question in questions {
        let fingerprint = super::recovery::fingerprint(question, &provider.model);
        let mut saved_tx = interviewer_transaction(pool, owner).await?;
        let saved: Option<Value> = repo::checkpoint(&mut saved_tx, session, &fingerprint).await?;
        saved_tx.commit().await?;
        let cached = saved.filter(|score| super::rubric::valid_checkpoint(score, question));
        let reused = cached.is_some();
        let score = match cached {
            Some(score) => score,
            None => evaluate(provider, question)
                .await
                .map_err(|error| super::recovery::QuestionFailure {
                    position: question["position"]
                        .as_u64()
                        .unwrap_or(0),
                    reason: super::recovery::safe_reason(&error),
                })?,
        };
        scores.push(score);
        let mut tx = interviewer_transaction(pool, owner).await?;
        let current: Value = repo::current_claim(&mut tx, session).await?;
        anyhow::ensure!(current["claim"] == json!(claim), "Assessment superseded");
        audit(
            &mut tx,
            owner,
            None,
            session,
            "assessment.question.scored",
            json!({"claim":claim,"result":scores.last(),"provisional":true,"input_hash":fingerprint,"reused":reused}),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Scoring progress unavailable"))?;
        tx.commit().await?;
    }
    Ok(scores)
}
