//! Blueprint, draft, critique, repair, deduplication and atomic publication.
use super::provider;
use super::{
    provider::AiProvider,
    types::{Blueprint, QuestionSet, cosine, fingerprint, vector_text},
};
use crate::infrastructure::interviewer_transaction;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{PROMPT_VERSION, quality, worker::Claim};
pub(super) async fn prepare(
    pool: &PgPool,
    provider: &AiProvider,
    claim: &Claim,
) -> anyhow::Result<()> {
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    let session=sqlx::query("SELECT s.job_id,j.description,s.duration_minutes,s.generation_error FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.id=$1 AND s.state IN ('invited','ready') AND s.expires_at>now()")
        .bind(claim.session).fetch_optional(&mut *tx).await?.ok_or_else(||anyhow::anyhow!("Session is no longer eligible for question preparation"))?;
    let jd: Uuid = session.get("job_id");
    let description: String = session.get("description");
    let duration: i32 = session.get("duration_minutes");
    let stored: Option<Value> = sqlx::query_scalar("SELECT blueprint FROM jobs WHERE id=$1")
        .bind(jd)
        .fetch_one(&mut *tx)
        .await?;
    let mut recent: Vec<String> = sqlx::query_scalar(
        "SELECT q.prompt FROM interview_questions q JOIN interview_sessions s ON s.id=q.session_id WHERE q.job_id=$1 ORDER BY s.created_at DESC,q.position,q.id LIMIT 30",
    )
    .bind(jd)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let initial_feedback = session
        .get::<Option<String>, _>("generation_error")
        .unwrap_or_default();
    tracing::info!(event = "task.phase", phase = "blueprint");
    let blueprint: Blueprint = if let Some(value) = stored {
        serde_json::from_value(value).map_err(|_| anyhow::anyhow!("Stored blueprint is invalid"))?
    } else {
        quality::blueprint(provider, &description, duration, &initial_feedback).await?
    };
    blueprint.validate(&description, provider.count)?;
    let mut repair_feedback: String = session
        .get::<Option<String>, _>("generation_error")
        .unwrap_or_default();
    let mut previous_draft: Option<Value> = None;
    let mut accepted = None;
    for draft_attempt in 1..=2 {
        tracing::info!(
            event = "task.phase",
            phase = "draft_and_validate",
            draft_attempt
        );
        let draft=provider.structured::<QuestionSet>("interview_questions",quality::QUESTION_INSTRUCTIONS,
            json!({"job_description":description,"blueprint":blueprint,"seed":claim.seed,"attempt":claim.attempts,"draft_attempt":draft_attempt,"duration_minutes":duration,"excluded_questions":recent,"repair_feedback":repair_feedback,"previous_draft":previous_draft})).await;
        let checked = async {
            let questions = draft?;
            previous_draft=Some(serde_json::to_value(&questions)?);
            questions.validate(&blueprint)?;
            questions.validate_budget(duration)?;
            let prompts:Vec<String>=questions.questions.iter().map(|q|q.prompt.clone()).collect();
            let vectors=provider.embeddings(&prompts).await?;
            for (i,a) in vectors.iter().enumerate(){for b in &vectors[..i]{anyhow::ensure!(cosine(a,b)<provider.similarity,"Generated set contains semantically duplicate questions");}}
            let mut tx=interviewer_transaction(pool,claim.recruiter).await?;
            let mut collisions=Vec::new();
            for (question,embedding) in questions.questions.iter().zip(&vectors) {
                let vector=vector_text(embedding)?;
                let collision:Option<String>=sqlx::query_scalar("SELECT prompt FROM interview_questions WHERE job_id=$1 AND (fingerprint=$2 OR (embedding_model=$3 AND embedding_dimensions=$4 AND 1-(embedding <=> $5::vector)>=$6)) ORDER BY id LIMIT 1").bind(jd).bind(fingerprint(&question.prompt)).bind(&provider.embedding_model).bind(provider.dimensions as i32).bind(vector).bind(provider.similarity).fetch_optional(&mut *tx).await?;
                if let Some(collision)=collision {collisions.push(collision);}
            }
            tx.commit().await?;
            let duplicated=!collisions.is_empty();
            for collision in collisions {if !recent.contains(&collision){recent.push(collision);}}
            anyhow::ensure!(!duplicated,"Question duplicated an existing JD-bank question; choose a different task from the exclusion list");
            let review=quality::review(provider,&description,&blueprint,&questions,duration).await?;
            Ok::<_,anyhow::Error>((questions,review,vectors))
        }
        .await;
        match checked {
            Ok(value) => {
                accepted = Some(value);
                break;
            }
            Err(error) => {
                let fatal = error
                    .downcast_ref::<provider::ProviderFailure>()
                    .is_some_and(|error| !error.retryable);
                repair_feedback = error.to_string();
                if draft_attempt == 2 || fatal {
                    return Err(error);
                }
            }
        }
    }
    let (questions, review, vectors) =
        accepted.ok_or_else(|| anyhow::anyhow!("No valid question set generated"))?;
    let mut tx = interviewer_transaction(pool, claim.recruiter).await?;
    let owned:Option<Uuid>=sqlx::query_scalar("SELECT id FROM interview_sessions WHERE generation_id=$1 AND generation_claim_token=$2 AND generation_status='running' AND generation_lease_until>clock_timestamp() FOR UPDATE")
        .bind(claim.id).bind(claim.token).fetch_optional(&mut *tx).await?;
    anyhow::ensure!(owned.is_some(), "Generation job lease was lost");
    let eligible:Option<Uuid>=sqlx::query_scalar("SELECT id FROM interview_sessions WHERE id=$1 AND state IN ('invited','ready') AND expires_at>now() FOR UPDATE").bind(claim.session).fetch_optional(&mut *tx).await?;
    anyhow::ensure!(
        eligible.is_some(),
        "Session is no longer eligible for question preparation"
    );
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(jd.to_string())
        .execute(&mut *tx)
        .await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM interview_questions WHERE session_id=$1")
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
        sqlx::query("INSERT INTO interview_questions(id,interviewer_id,session_id,job_id,position,kind,difficulty,prompt,rubric,generation_id,competency_id,fingerprint,embedding,embedding_model,embedding_dimensions) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13::vector,$14,$15)")
            .bind(Uuid::new_v4()).bind(claim.recruiter).bind(claim.session).bind(jd).bind((index+1) as i32)
            .bind(serde_json::to_value(&question.kind)?.as_str().unwrap_or("text")).bind(serde_json::to_value(&question.difficulty)?.as_str().unwrap_or("medium")).bind(&question.prompt)
            .bind(json!({"expected_answer":question.worked_reference(),"reference_overview":question.expected_answer,"reference_sections":sections,"estimated_minutes":question.estimated_minutes,"assumptions":question.assumptions,"acceptable_alternatives":question.acceptable_alternatives,"common_mistakes":question.common_mistakes,"coding":question.coding,"quality_review":review.questions.iter().find(|r|r.position==index+1),"criteria":criteria,"max_score":10,"competency_id":question.competency_id,"model":provider.model,"prompt_version":PROMPT_VERSION}))
            .bind(claim.id).bind(&question.competency_id).bind(hash).bind(vector).bind(&provider.embedding_model).bind(provider.dimensions as i32).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE interview_sessions SET generation_status='completed',generation_claim_token=NULL,generation_lease_until=NULL,generation_error=NULL WHERE id=$1 AND generation_claim_token=$2")
        .bind(claim.session).bind(claim.token).execute(&mut *tx).await?;
    // No secret, candidate answer, or raw provider output enters the audit record.
    sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'questions.prepared',$4)")
        .bind(Uuid::new_v4()).bind(claim.recruiter).bind(claim.session).bind(json!({"actor":"worker","count":questions.questions.len(),"model":provider.model,"embedding_model":provider.embedding_model,"prompt_version":PROMPT_VERSION,"seed":claim.seed,"quality_review":"passed","reference_version":"worked-answer-v2"})).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
