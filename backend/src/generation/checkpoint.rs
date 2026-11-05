//! Save one fully validated question atomically; mark the set ready only after the last.
use super::{
    PROMPT_VERSION,
    pipeline::Preparation,
    quality::QuestionReview,
    types::{Question, fingerprint, vector_text},
};
use crate::infrastructure::interviewer_transaction;
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) async fn save(
    context: &Preparation<'_>,
    question: &Question,
    review: &QuestionReview,
    embedding: &[f32],
) -> anyhow::Result<()> {
    let claim = context.claim;
    let mut tx = interviewer_transaction(context.pool, claim.recruiter).await?;
    let owned: Option<Uuid> = if claim.is_pool {
        sqlx::query_scalar("SELECT id FROM question_sets WHERE id=$1 AND claim_token=$2 AND status='running' AND lease_until>clock_timestamp() FOR UPDATE")
            .bind(claim.id).bind(claim.token).fetch_optional(&mut *tx).await?
    } else {
        sqlx::query_scalar("SELECT id FROM interview_sessions WHERE generation_id=$1 AND generation_claim_token=$2 AND generation_status='running' AND generation_lease_until>clock_timestamp() AND state IN ('invited','ready') AND expires_at>clock_timestamp() FOR UPDATE")
            .bind(claim.id).bind(claim.token).fetch_optional(&mut *tx).await?
    };
    anyhow::ensure!(
        owned.is_some(),
        "Generation job lease was lost or target expired"
    );
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(context.job.to_string())
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM interview_questions WHERE session_id=$1 OR set_id=$1",
    )
    .bind(claim.session)
    .fetch_one(&mut *tx)
    .await?;
    anyhow::ensure!(
        count == context.position as i64 - 1,
        "Question checkpoint changed; resume from saved progress"
    );
    let blueprint: Option<Value> = sqlx::query_scalar("SELECT blueprint FROM jobs WHERE id=$1")
        .bind(context.job)
        .fetch_one(&mut *tx)
        .await?;
    anyhow::ensure!(
        blueprint == Some(serde_json::to_value(context.job_blueprint)?),
        "Job blueprint changed during question preparation"
    );
    let vector = vector_text(embedding)?;
    let hash = fingerprint(&question.prompt);
    let duplicate:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM interview_questions WHERE job_id=$1 AND (fingerprint=$2 OR (embedding_model=$3 AND embedding_dimensions=$4 AND 1-(embedding <=> $5::vector)>=$6)))")
        .bind(context.job).bind(&hash).bind(&context.provider.embedding_model).bind(context.provider.dimensions as i32).bind(&vector).bind(context.provider.similarity).fetch_one(&mut *tx).await?;
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
    sqlx::query("INSERT INTO interview_questions(id,interviewer_id,session_id,job_id,position,kind,difficulty,prompt,rubric,generation_id,competency_id,fingerprint,embedding,embedding_model,embedding_dimensions,set_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13::vector,$14,$15,$16)")
        .bind(Uuid::new_v4()).bind(claim.recruiter).bind(if claim.is_pool {None}else{Some(claim.session)}).bind(context.job).bind(context.position as i32)
        .bind(serde_json::to_value(&question.kind)?.as_str().unwrap_or("text")).bind(serde_json::to_value(&question.difficulty)?.as_str().unwrap_or("medium")).bind(&question.prompt).bind(rubric).bind(claim.id).bind(&question.competency_id).bind(hash).bind(vector).bind(&context.provider.embedding_model).bind(context.provider.dimensions as i32).bind(if claim.is_pool {Some(claim.id)}else{None}).execute(&mut *tx).await?;
    let total = context.job_blueprint.slots.len();
    let complete = context.position == total;
    if claim.is_pool {
        sqlx::query("UPDATE question_sets SET status=CASE WHEN $3 THEN 'completed' ELSE 'running' END,claim_token=CASE WHEN $3 THEN NULL ELSE claim_token END,lease_until=CASE WHEN $3 THEN NULL ELSE lease_until END,last_error=NULL WHERE id=$1 AND claim_token=$2")
            .bind(claim.id).bind(claim.token).bind(complete).execute(&mut *tx).await?;
    } else {
        sqlx::query("UPDATE interview_sessions SET generation_status=CASE WHEN $3 THEN 'completed' ELSE 'running' END,generation_claim_token=CASE WHEN $3 THEN NULL ELSE generation_claim_token END,generation_lease_until=CASE WHEN $3 THEN NULL ELSE generation_lease_until END,generation_error=NULL WHERE id=$1 AND generation_claim_token=$2")
            .bind(claim.session).bind(claim.token).bind(complete).execute(&mut *tx).await?;
    }
    sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'question.prepared',$4)")
        .bind(Uuid::new_v4()).bind(claim.recruiter).bind(claim.session).bind(json!({"actor":"worker","position":context.position,"total":total,"prompt_version":PROMPT_VERSION})).execute(&mut *tx).await?;
    if complete {
        sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'questions.prepared',$4)")
            .bind(Uuid::new_v4()).bind(claim.recruiter).bind(claim.session).bind(json!({"actor":"worker","count":total,"model":context.provider.model,"embedding_model":context.provider.embedding_model,"prompt_version":PROMPT_VERSION,"seed":claim.seed,"quality_review":"passed","reference_version":"worked-answer-v2","source":if claim.is_pool {"job_pool"}else{"session"}})).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}
