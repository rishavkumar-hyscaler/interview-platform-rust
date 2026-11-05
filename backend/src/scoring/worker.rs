//! Claim and score submitted sessions, then commit validated results.
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
    let owners: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer'")
        .fetch_all(pool)
        .await?;
    for owner in owners {
        let mut tx = interviewer_transaction(pool, owner).await?;
        let exhausted:Option<Uuid>=sqlx::query_scalar("SELECT s.id FROM interview_sessions s WHERE s.state='scoring' AND NOT EXISTS(SELECT 1 FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.started' AND a.created_at>now()-interval '15 minutes') AND (SELECT count(*) FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.started' AND a.created_at>coalesce((SELECT max(r.created_at) FROM audit_events r WHERE r.entity_id=s.id AND r.action='assessment.retry'),'-infinity'::timestamptz))>=3 FOR UPDATE SKIP LOCKED LIMIT 1").fetch_optional(&mut *tx).await?;
        if let Some(id) = exhausted {
            sqlx::query("UPDATE interview_sessions SET state='submitted' WHERE id=$1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
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

        let session:Option<Uuid> = sqlx::query_scalar("SELECT s.id FROM interview_sessions s WHERE s.state IN ('submitted','scoring') AND NOT EXISTS(SELECT 1 FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.completed') AND (SELECT count(*) FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.started' AND a.created_at>coalesce((SELECT max(r.created_at) FROM audit_events r WHERE r.entity_id=s.id AND r.action='assessment.retry'),'-infinity'::timestamptz))<3 AND NOT EXISTS(SELECT 1 FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.started' AND a.created_at>now()-interval '15 minutes') ORDER BY s.submitted_at FOR UPDATE SKIP LOCKED LIMIT 1").fetch_optional(&mut *tx).await?;
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
        sqlx::query("UPDATE interview_sessions SET state='scoring' WHERE id=$1")
            .bind(session)
            .execute(&mut *tx)
            .await?;
        let questions:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',q.id,'position',q.position,'prompt',q.prompt,'rubric',q.rubric,'competency_id',q.competency_id,'answer',coalesce(a.content,''),'recordings',(SELECT count(*) FROM audit_events m WHERE m.entity_id=q.session_id AND m.action='answer.recorded' AND m.metadata->>'question_id'=q.id::text)) FROM interview_questions q LEFT JOIN interview_answers a ON a.question_id=q.id WHERE q.session_id=$1 ORDER BY q.position").bind(session).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        let mut task = crate::observability::TaskLog::start("assessment.score", session);
        let started = std::time::Instant::now();
        let result = assess(provider, &questions, pool, owner, session, claim)
            .instrument(task.span())
            .await;
        tracing::info!(%session,elapsed_ms=started.elapsed().as_millis() as u64,success=result.is_ok(),"assessment attempt finished");
        let mut tx = interviewer_transaction(pool, owner).await?;
        sqlx::query("SELECT id FROM interview_sessions WHERE id=$1 FOR UPDATE")
            .bind(session)
            .fetch_one(&mut *tx)
            .await?;
        let current:Value=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE entity_id=$1 AND action='assessment.started' ORDER BY created_at DESC,id DESC LIMIT 1").bind(session).fetch_one(&mut *tx).await?;
        if current["claim"] != json!(claim) {
            tx.commit().await?;
            return Ok(true);
        }
        let success = result.is_ok();
        match result {
            Ok(scores) => {
                let blueprint:Option<Value>=sqlx::query_scalar("SELECT j.blueprint FROM jobs j JOIN interview_sessions s ON s.job_id=j.id WHERE s.id=$1").bind(session).fetch_one(&mut *tx).await?;
                let profile = competency_profile(&scores, blueprint.as_ref());

                audit(&mut tx,owner,None,session,"assessment.completed",json!({"questions":scores,"competency_profile":profile,"model":provider.model,"version":"rubric-v1","human_review_required":true})).await.map_err(|_| anyhow::anyhow!("Assessment audit failed"))?;
                sqlx::query("UPDATE interview_sessions SET state='completed' WHERE id=$1")
                    .bind(session)
                    .execute(&mut *tx)
                    .await?;
            }
            Err(_) => {
                tracing::warn!(%session,"assessment failed validation or provider request");
                audit(&mut tx,owner,None,session,"assessment.failed",json!({"claim":claim,"reason":"Provider request or evidence validation failed"})).await.map_err(|_| anyhow::anyhow!("Assessment audit failed"))?;
                sqlx::query("UPDATE interview_sessions SET state='submitted' WHERE id=$1")
                    .bind(session)
                    .execute(&mut *tx)
                    .await?;
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
        scores.push(evaluate(provider, question).await?);
        let mut tx = interviewer_transaction(pool, owner).await?;
        let current:Value=sqlx::query_scalar("SELECT metadata FROM audit_events WHERE entity_id=$1 AND action='assessment.started' ORDER BY created_at DESC,id DESC LIMIT 1").bind(session).fetch_one(&mut *tx).await?;
        anyhow::ensure!(current["claim"] == json!(claim), "Assessment superseded");
        audit(
            &mut tx,
            owner,
            None,
            session,
            "assessment.question.scored",
            json!({"claim":claim,"result":scores.last(),"provisional":true}),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Scoring progress unavailable"))?;
        tx.commit().await?;
    }
    Ok(scores)
}
