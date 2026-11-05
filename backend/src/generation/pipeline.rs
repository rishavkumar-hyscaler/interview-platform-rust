//! Blueprint, draft, critique, repair, deduplication and atomic publication.
use super::{provider::AiProvider, types::Blueprint};
use crate::infrastructure::interviewer_transaction;
use serde_json::Value;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{PROMPT_VERSION, quality, worker::Claim};
pub(super) struct Preparation<'a> {
    pub pool: &'a PgPool,
    pub provider: &'a AiProvider,
    pub claim: &'a Claim,
    pub job: Uuid,
    pub description: &'a str,
    pub blueprint: &'a Blueprint,
    pub duration: i32,
    pub position: usize,
    pub job_blueprint: &'a Blueprint,
}
pub(super) async fn prepare(
    pool: &PgPool,
    provider: &AiProvider,
    claim: &Claim,
) -> anyhow::Result<()> {
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    let session = if claim.is_pool {
        sqlx::query("SELECT p.job_id,j.description,p.duration_minutes,p.last_error AS generation_error FROM question_sets p JOIN jobs j ON j.id=p.job_id WHERE p.id=$1")
            .bind(claim.id).fetch_optional(&mut *tx).await?
    } else {
        sqlx::query("SELECT s.job_id,j.description,s.duration_minutes,s.generation_error FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.id=$1 AND s.state IN ('invited','ready') AND s.expires_at>now()")
            .bind(claim.session).fetch_optional(&mut *tx).await?
    }.ok_or_else(|| anyhow::anyhow!("Question preparation target is no longer eligible"))?;
    let jd: Uuid = session.get("job_id");
    let description: String = session.get("description");
    let duration: i32 = session.get("duration_minutes");
    let stored: Option<Value> = sqlx::query_scalar("SELECT blueprint FROM jobs WHERE id=$1")
        .bind(jd)
        .fetch_one(&mut *tx)
        .await?;
    let recent: Vec<String> = sqlx::query_scalar(
        "SELECT q.prompt FROM interview_questions q WHERE q.job_id=$1 ORDER BY q.id DESC LIMIT 30",
    )
    .bind(jd)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let initial_feedback = session
        .get::<Option<String>, _>("generation_error")
        .unwrap_or_default();
    tracing::info!(event = "task.phase", phase = "blueprint");
    let mut blueprint: Blueprint = if let Some(value) = stored {
        serde_json::from_value(value).map_err(|_| anyhow::anyhow!("Stored blueprint is invalid"))?
    } else {
        quality::blueprint(provider, &description, duration, &initial_feedback).await?
    };
    blueprint.validate(&description, provider.count)?;
    // Cache the validated blueprint before expensive stages; failed references do
    // not require extracting competencies again on the next authorized retry.
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    let current: Option<Value> =
        sqlx::query_scalar("SELECT blueprint FROM jobs WHERE id=$1 FOR UPDATE")
            .bind(jd)
            .fetch_one(&mut *tx)
            .await?;
    if let Some(current) = current {
        blueprint = serde_json::from_value(current)?;
        blueprint.validate(&description, provider.count)?;
    } else {
        sqlx::query("UPDATE jobs SET blueprint=$2,blueprint_model=$3,blueprint_prompt_version=$4 WHERE id=$1")
            .bind(jd).bind(serde_json::to_value(&blueprint)?).bind(&provider.model).bind(PROMPT_VERSION).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    let saved = sqlx::query("SELECT position,coalesce((rubric->>'estimated_minutes')::integer,1) AS minutes FROM interview_questions WHERE session_id=$1 OR set_id=$1 ORDER BY position")
        .bind(claim.session).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    for (index, row) in saved.iter().enumerate() {
        anyhow::ensure!(
            row.get::<i32, _>("position") == index as i32 + 1,
            "Saved question positions are not contiguous"
        );
    }
    let total = blueprint.slots.len();
    anyhow::ensure!(
        saved.len() < total,
        "All questions are already saved; task status needs reconciliation"
    );
    let mut remaining = duration
        - saved
            .iter()
            .map(|row| row.get::<i32, _>("minutes"))
            .sum::<i32>();
    let mut excluded = recent;
    for index in saved.len()..total {
        let position = index + 1;
        let left = (total - index) as i32;
        anyhow::ensure!(
            remaining >= left,
            "Remaining interview duration cannot fit the unsaved questions"
        );
        let budget = (remaining / left).clamp(1, 30);
        let mut single_blueprint = blueprint.clone();
        single_blueprint.slots = vec![blueprint.slots[index].clone()];
        let context = Preparation {
            pool,
            provider,
            claim,
            job: jd,
            description: &description,
            blueprint: &single_blueprint,
            duration: budget,
            position,
            job_blueprint: &blueprint,
        };
        let seconds = match std::env::var("GENERATION_TASK_TIMEOUT_SECONDS") {
            Ok(value) => value.parse::<u64>().unwrap_or(300).clamp(30, 900),
            Err(_) => 300,
        };
        tracing::info!(
            event = "task.phase",
            phase = "question.begin",
            position,
            total
        );
        let prepared = async {
            let (questions, review, vectors) =
                super::drafts::prepare(&context, excluded.clone(), &initial_feedback).await?;
            let question = &questions.questions[0];
            super::checkpoint::save(&context, question, &review.questions[0], &vectors[0]).await?;
            Ok::<_, anyhow::Error>((question.prompt.clone(), question.estimated_minutes))
        };
        let (prompt,minutes)=match tokio::time::timeout(std::time::Duration::from_secs(seconds),prepared).await {
            Ok(result)=>result?,
            Err(_)=>return Err(super::PreparationRejected(format!("Question {position} exceeded its preparation time budget; saved questions are preserved")).into()),
        };
        remaining -= minutes as i32;
        excluded.push(prompt);
        tracing::info!(
            event = "task.phase",
            phase = "question.saved",
            position,
            total
        );
    }
    Ok(())
}
