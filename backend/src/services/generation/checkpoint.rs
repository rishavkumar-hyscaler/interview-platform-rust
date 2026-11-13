//! Save one fully validated question atomically; mark the set ready only after the last.
use super::{
    PROMPT_VERSION,
    pipeline::Preparation,
    quality::QuestionReview,
    types::{Question, fingerprint, vector_text},
    variation::Variation,
};
use crate::infrastructure::interviewer_transaction;
use crate::repositories::generation_workflow_repository as repo;
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) async fn save(
    context: &Preparation<'_>,
    question: &Question,
    review: &QuestionReview,
    embedding: &[f32],
    variation: &Variation,
) -> anyhow::Result<()> {
    let claim = context.claim;
    let mut tx = interviewer_transaction(context.pool, claim.recruiter).await?;
    let owned: Option<Uuid> = repo::lock_owned_set(&mut tx, claim.id, claim.token).await?;
    anyhow::ensure!(
        owned.is_some(),
        "Generation job lease was lost or target expired"
    );
    repo::lock_question_bank(&mut tx, context.job.to_string()).await?;
    let count: i64 = repo::question_count(&mut tx, claim.id).await?;
    anyhow::ensure!(
        count == context.position as i64 - 1,
        "Question checkpoint changed; resume from saved progress"
    );
    let blueprint: Option<Value> = repo::blueprint(&mut tx, context.job).await?;
    anyhow::ensure!(
        blueprint == Some(serde_json::to_value(context.job_blueprint)?),
        "Job blueprint changed during question preparation"
    );
    let vector = vector_text(embedding)?;
    let hash = fingerprint(&question.prompt);
    let duplicate: bool = repo::duplicate_exists(
        &mut tx,
        context.job,
        &hash,
        &context.provider.embedding_model,
        context.provider.dimensions as i32,
        &vector,
        context.provider.similarity,
    )
    .await?;
    anyhow::ensure!(
        !duplicate,
        "Question duplicated an existing JD-bank question at checkpoint"
    );
    let mut criteria = serde_json::to_value(&question.criteria)?;
    for (value, criterion) in criteria
        .as_array_mut()
        .expect("criteria array")
        .iter_mut()
        .zip(&question.criteria)
    {
        value["reference_evidence"] = json!(criterion.reference_answer);
        value["reference_source"] = json!(format!("reference_sections.{}", criterion.id));
    }
    let sections: Vec<Value> = question
        .criteria
        .iter()
        .map(|criterion| json!({"criterion_id":criterion.id,"answer":criterion.reference_answer}))
        .collect();
    let rubric = json!({"expected_answer":question.worked_reference(),"reference_overview":question.expected_answer,"reference_sections":sections,"estimated_minutes":question.estimated_minutes,"assumptions":question.assumptions,"acceptable_alternatives":question.acceptable_alternatives,"common_mistakes":question.common_mistakes,"coding":question.coding,"quality_review":review,"criteria":criteria,"max_score":10,"competency_id":question.competency_id,"model":context.provider.model,"prompt_version":PROMPT_VERSION});
    repo::insert_question(
        &mut tx,
        crate::models::generation::PreparedQuestionInsert {
            id: Uuid::new_v4(),
            owner: claim.recruiter,
            job: context.job,
            position: context.position as i32,
            kind: serde_json::to_value(&question.kind)?
                .as_str()
                .unwrap_or("text"),
            difficulty: serde_json::to_value(&question.difficulty)?
                .as_str()
                .unwrap_or("medium"),
            prompt: &question.prompt,
            rubric,
            competency: &question.competency_id,
            fingerprint: hash,
            vector,
            model: &context.provider.embedding_model,
            dimensions: context.provider.dimensions as i32,
            set: claim.id,
        },
    )
    .await?;
    let total = context.planned_total;
    let complete = context.position == total;
    repo::advance_checkpoint(&mut tx, claim.id, claim.token, complete).await?;
    repo::record_question(&mut tx,Uuid::new_v4(),claim.recruiter,claim.id,json!({"actor":"worker","position":context.position,"total":total,"prompt_version":PROMPT_VERSION,"variation":variation,"parameterized":variation.applied(&question.prompt)})).await?;
    if complete {
        crate::services::invitation_service::publish_prepared(&mut tx, claim.recruiter, claim.id)
            .await
            .map_err(|error| anyhow::anyhow!(error.1))?;
        repo::record_completion(&mut tx,Uuid::new_v4(),claim.recruiter,claim.id,json!({"actor":"worker","count":total,"model":context.provider.model,"embedding_model":context.provider.embedding_model,"prompt_version":PROMPT_VERSION,"seed":claim.seed,"quality_review":"passed","reference_version":"worked-answer-v2","source":"candidate_preparation"})).await?;
    }
    tx.commit().await?;
    Ok(())
}
