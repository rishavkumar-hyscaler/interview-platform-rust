//! Blueprint, draft, critique, repair, deduplication and atomic publication.
use super::{provider::AiProvider, types::Blueprint};
use crate::infrastructure::interviewer_transaction;
use crate::repositories::generation_workflow_repository as repo;
use serde_json::Value;
use sqlx::PgPool;
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
    pub planned_scope: &'a str,
    pub planned_total: usize,
    pub job_blueprint: &'a Blueprint,
}
pub(super) async fn prepare(
    pool: &PgPool,
    provider: &AiProvider,
    claim: &Claim,
) -> anyhow::Result<()> {
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    let session = repo::preparation_job(&mut tx, claim.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Question set no longer exists"))?;
    let jd: Uuid = session.job_id;
    let description: String = session.description;
    let duration: i32 = session.duration_minutes;
    let stored: Option<Value> = repo::blueprint(&mut tx, jd).await?;
    let recent: Vec<String> = repo::recent_prompts(&mut tx, jd).await?;
    tx.commit().await?;
    let initial_feedback = session
        .generation_error
        .unwrap_or_default();
    tracing::info!(event = "task.phase", phase = "blueprint");
    let mut blueprint: Blueprint = if let Some(value) = stored {
        serde_json::from_value(value).map_err(|_| anyhow::anyhow!("Stored blueprint is invalid"))?
    } else {
        quality::blueprint(provider, &description, duration, &initial_feedback).await?
    };
    blueprint.validate(&description, blueprint.slots.len())?;
    // Cache the validated blueprint before expensive stages; failed references do
    // not require extracting competencies again on the next authorized retry.
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    let current: Option<Value> = repo::lock_blueprint(&mut tx, jd).await?;
    if let Some(current) = current {
        blueprint = serde_json::from_value(current)?;
        blueprint.validate(&description, blueprint.slots.len())?;
    } else {
        repo::save_blueprint(
            &mut tx,
            jd,
            serde_json::to_value(&blueprint)?,
            &provider.model,
            PROMPT_VERSION,
        )
        .await?;
    }
    tx.commit().await?;
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    let saved = repo::saved_question_times(&mut tx, claim.id).await?;
    tx.commit().await?;
    for (index, row) in saved.iter().enumerate() {
        anyhow::ensure!(
            row.position == index as i32 + 1,
            "Saved question positions are not contiguous"
        );
    }
    let saved_minutes: Vec<i32> = saved
        .iter()
        .map(|row| row.minutes)
        .collect();
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    repo::lock_job(&mut tx, jd).await?;
    let stored_plan: Option<Value> = repo::plan(&mut tx, claim.id).await?;
    let plan: super::plan::Plan = match stored_plan {
        Some(value) => serde_json::from_value(value)?,
        None => {
            let plan = super::plan::Plan::build(&blueprint, duration, &saved_minutes)?;
            repo::record_plan(
                &mut tx,
                Uuid::new_v4(),
                claim.recruiter,
                claim.id,
                serde_json::json!({"version":1,"plan":plan}),
            )
            .await?;
            plan
        }
    };
    plan.validate(&saved_minutes)?;
    anyhow::ensure!(
        plan.duration_minutes == duration,
        "Interview duration changed after planning"
    );
    tx.commit().await?;
    let total = plan.blueprint.slots.len();
    anyhow::ensure!(
        saved.len() < total,
        "All planned questions are already saved; task status needs reconciliation"
    );
    let mut excluded = recent;
    for index in saved.len()..total {
        let position = index + 1;
        let budget = plan.allocations[index].minutes;
        let mut single_blueprint = plan.blueprint.clone();
        single_blueprint.slots = vec![plan.blueprint.slots[index].clone()];
        let context = Preparation {
            pool,
            provider,
            claim,
            job: jd,
            description: &description,
            blueprint: &single_blueprint,
            duration: budget,
            position,
            planned_scope: &plan.allocations[index].scope,
            planned_total: total,
            job_blueprint: &blueprint,
        };
        let seconds = match std::env::var("GENERATION_TASK_TIMEOUT_SECONDS") {
            Ok(value) => value
                .parse::<u64>()
                .unwrap_or(300)
                .clamp(30, 900),
            Err(_) => 300,
        };
        tracing::info!(
            event = "task.phase",
            phase = "question.begin",
            position,
            total
        );
        let prepared = async {
            let (questions, review, vectors, variation) =
                super::drafts::prepare(&context, excluded.clone(), &initial_feedback).await?;
            let question = &questions.questions[0];
            super::checkpoint::save(
                &context,
                question,
                &review.questions[0],
                &vectors[0],
                &variation,
            )
            .await?;
            Ok::<_, anyhow::Error>(question.prompt.clone())
        };
        let prompt=match tokio::time::timeout(std::time::Duration::from_secs(seconds),prepared).await {
            Ok(result)=>result?,
            Err(_)=>return Err(super::PreparationRejected(format!("Question {position} exceeded its preparation time budget; saved questions are preserved")).into()),
        };
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
