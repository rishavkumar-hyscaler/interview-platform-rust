//! Blueprint, draft, critique, repair, deduplication and atomic publication.
use super::{
    provider::AiProvider,
    types::{Blueprint, fingerprint, vector_text},
};
use crate::infrastructure::interviewer_transaction;
use serde_json::{Value, json};
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
}
pub(super) async fn prepare(
    pool: &PgPool,
    provider: &AiProvider,
    claim: &Claim,
) -> anyhow::Result<()> {
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    let session = if claim.is_pool {
        sqlx::query("SELECT p.job_id,j.description,p.duration_minutes,p.last_error AS generation_error FROM question_sets p JOIN jobs j ON j.id=p.job_id WHERE p.id=$1 AND p.assigned_session_id IS NULL")
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
    let context = Preparation {
        pool,
        provider,
        claim,
        job: jd,
        description: &description,
        blueprint: &blueprint,
        duration,
    };
    let (prompts, vectors) = super::drafts::prepare(&context, recent, &initial_feedback).await?;
    // No reference-answer request is made until every prompt passes the gate.
    let (questions, review) = super::references::prepare(&context, prompts).await?;
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    let owned: Option<Uuid> = if claim.is_pool {
        sqlx::query_scalar("SELECT id FROM question_sets WHERE id=$1 AND claim_token=$2 AND status='running' AND lease_until>clock_timestamp() AND assigned_session_id IS NULL FOR UPDATE")
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
        .bind(jd.to_string())
        .execute(&mut *tx)
        .await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM interview_questions WHERE session_id=$1 OR set_id=$1",
    )
    .bind(claim.session)
    .fetch_one(&mut *tx)
    .await?;
    anyhow::ensure!(count == 0, "Session questions already exist");
    let incompatible:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM interview_questions WHERE job_id=$1 AND (embedding_model<>$2 OR embedding_dimensions<>$3))")
        .bind(jd).bind(&provider.embedding_model).bind(provider.dimensions as i32).fetch_one(&mut *tx).await?;
    anyhow::ensure!(
        !incompatible,
        "Existing question bank requires embedding migration before changing the embedding model"
    );
    let current: Option<Value> = sqlx::query_scalar("SELECT blueprint FROM jobs WHERE id=$1")
        .bind(jd)
        .fetch_one(&mut *tx)
        .await?;
    if let Some(current) = current {
        anyhow::ensure!(
            current == serde_json::to_value(&blueprint)?,
            "Another session established a different blueprint; retrying with the stored blueprint"
        );
    } else {
        sqlx::query("UPDATE jobs SET blueprint=$2,blueprint_model=$3,blueprint_prompt_version=$4 WHERE id=$1")
            .bind(jd).bind(serde_json::to_value(&blueprint)?).bind(&provider.model).bind(PROMPT_VERSION).execute(&mut *tx).await?;
    }
    tracing::info!(event = "task.phase", phase = "publish");
    for (index, (question, embedding)) in questions.questions.iter().zip(&vectors).enumerate() {
        let vector = vector_text(embedding)?;
        let hash = fingerprint(&question.prompt);
        let duplicate:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM interview_questions WHERE job_id=$1 AND (fingerprint=$2 OR (embedding_model=$3 AND embedding_dimensions=$4 AND 1-(embedding <=> $5::vector)>=$6)))")
            .bind(jd).bind(&hash).bind(&provider.embedding_model).bind(provider.dimensions as i32).bind(&vector).bind(provider.similarity).fetch_one(&mut *tx).await?;
        anyhow::ensure!(
            !duplicate,
            "Question duplicated an existing JD-bank question; generating a new variation on retry"
        );
        let mut criteria = serde_json::to_value(&question.criteria)?;
        for (value, criterion) in criteria
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(&question.criteria)
        {
            value["reference_evidence"] = json!(criterion.reference_answer);
            value["reference_source"] = json!(format!("reference_sections.{}", criterion.id));
        }
        let sections:Vec<Value>=question.criteria.iter().map(|criterion|json!({"criterion_id":criterion.id,"answer":criterion.reference_answer})).collect();
        sqlx::query("INSERT INTO interview_questions(id,interviewer_id,session_id,job_id,position,kind,difficulty,prompt,rubric,generation_id,competency_id,fingerprint,embedding,embedding_model,embedding_dimensions,set_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13::vector,$14,$15,$16)")
            .bind(Uuid::new_v4()).bind(claim.recruiter).bind(if claim.is_pool { None } else { Some(claim.session) }).bind(jd).bind((index+1) as i32)
            .bind(serde_json::to_value(&question.kind)?.as_str().unwrap_or("text")).bind(serde_json::to_value(&question.difficulty)?.as_str().unwrap_or("medium")).bind(&question.prompt)
            .bind(json!({"expected_answer":question.worked_reference(),"reference_overview":question.expected_answer,"reference_sections":sections,"estimated_minutes":question.estimated_minutes,"assumptions":question.assumptions,"acceptable_alternatives":question.acceptable_alternatives,"common_mistakes":question.common_mistakes,"coding":question.coding,"quality_review":review.questions.iter().find(|r|r.position==index+1),"criteria":criteria,"max_score":10,"competency_id":question.competency_id,"model":provider.model,"prompt_version":PROMPT_VERSION}))
            .bind(claim.id).bind(&question.competency_id).bind(hash).bind(vector).bind(&provider.embedding_model).bind(provider.dimensions as i32).bind(if claim.is_pool { Some(claim.id) } else { None }).execute(&mut *tx).await?;
    }
    if claim.is_pool {
        sqlx::query("UPDATE question_sets SET status='completed',claim_token=NULL,lease_until=NULL,last_error=NULL WHERE id=$1 AND claim_token=$2")
            .bind(claim.id).bind(claim.token).execute(&mut *tx).await?;
    } else {
        sqlx::query("UPDATE interview_sessions SET generation_status='completed',generation_claim_token=NULL,generation_lease_until=NULL,generation_error=NULL WHERE id=$1 AND generation_claim_token=$2")
        .bind(claim.session).bind(claim.token).execute(&mut *tx).await?;
    }
    // No secret, candidate answer, or raw provider output enters the audit record.
    sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'questions.prepared',$4)")
        .bind(Uuid::new_v4()).bind(claim.recruiter).bind(claim.session).bind(json!({"actor":"worker","count":questions.questions.len(),"model":provider.model,"embedding_model":provider.embedding_model,"prompt_version":PROMPT_VERSION,"seed":claim.seed,"quality_review":"passed","reference_version":"worked-answer-v2","source":if claim.is_pool {"job_pool"} else {"session"}})).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
