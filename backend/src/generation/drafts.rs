//! Generate one complete question, deduplicate it, then independently review it.
use super::{
    pipeline::Preparation,
    quality,
    types::{PromptSet, QuestionPrompt, QuestionSet, fingerprint, vector_text},
};
use crate::infrastructure::interviewer_transaction;
use serde_json::json;
use sqlx::Row;

#[derive(Debug)]
struct DuplicateQuestion;
impl std::fmt::Display for DuplicateQuestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Question duplicated an existing or excluded JD-bank question; choose a different task from the exclusion list")
    }
}
impl std::error::Error for DuplicateQuestion {}

pub(super) async fn prepare(
    context: &Preparation<'_>,
    mut excluded: Vec<String>,
    initial_feedback: &str,
) -> anyhow::Result<(QuestionSet, quality::Review, Vec<Vec<f32>>)> {
    let diagnostics = super::diagnostics::Scope {
        pool: context.pool.clone(),
        owner: context.claim.recruiter,
        entity: context.claim.session,
        position: Some(context.position),
    };
    let mut feedback = initial_feedback.to_string();
    let mut previous = None;
    let mut duplicate_replaced = false;
    let mut content_repaired = false;
    let mut cached_embedding: Option<(String, Vec<f32>)> = None;
    // A duplicate replacement does not consume the one content repair.
    // Never exceed three drafts or regenerate saved questions.
    for attempt in 1..=3 {
        tracing::info!(event = "task.phase", phase = "question.generate", attempt);
        let response = context.provider.structured::<QuestionSet>(
            "interview_questions", quality::QUESTION_INSTRUCTIONS,
            json!({"job_description":context.description,"blueprint":context.blueprint,
                "positions":[context.position],"seed":context.claim.seed,"attempt":context.claim.attempts,
                "draft_attempt":attempt,"duration_minutes":context.duration,
                "expected_output_format":quality::question_format(&context.blueprint.slots[0], context.duration),
                "excluded_questions":excluded,"repair_feedback":feedback,"previous_draft":previous})
        ).await;
        let checked = async {
            let questions = response?;
            previous = Some(serde_json::to_value(&questions)?);
            anyhow::ensure!(
                questions.questions.len() == 1,
                "Generate exactly one question per checkpoint"
            );
            questions.validate_at(context.blueprint, context.position)?;
            questions.validate_budget(context.duration)?;
            let prompts = PromptSet {
                questions: questions
                    .questions
                    .iter()
                    .map(|q| QuestionPrompt {
                        competency_id: q.competency_id.clone(),
                        kind: q.kind.clone(),
                        difficulty: q.difficulty.clone(),
                        prompt: q.prompt.clone(),
                        estimated_minutes: q.estimated_minutes,
                    })
                    .collect(),
            };
            prompts.validate(context.blueprint, context.duration)?;
            if excluded
                .iter()
                .any(|old| fingerprint(old) == fingerprint(&prompts.questions[0].prompt))
            {
                return Err(DuplicateQuestion.into());
            }
            exact_duplicates(context, &prompts, &mut excluded).await?;
            let texts = vec![prompts.questions[0].prompt.clone()];
            let vectors = match &cached_embedding {
                Some((prompt, vector)) if prompt == &texts[0] => vec![vector.clone()],
                _ => context.provider.embeddings(&texts).await?,
            };
            cached_embedding = Some((texts[0].clone(), vectors[0].clone()));
            semantic_duplicates(context, &prompts, &vectors, &mut excluded).await?;
            tracing::info!(
                event = "task.phase",
                phase = "question.review",
                position = context.position
            );
            let review = quality::review(
                context.provider,
                context.description,
                context.blueprint,
                &questions,
                context.duration,
                context.position,
            )
            .await?;
            Ok::<_, anyhow::Error>((questions, review, vectors))
        }
        .await;
        match checked {
            Ok(approved) => return Ok(approved),
            Err(error) => {
                diagnostics
                    .rejected("question", attempt, &error, &previous)
                    .await;
                if quality::prompt_defect(&error)
                    && let Some(prompt) = previous
                        .as_ref()
                        .and_then(|v| v["questions"][0]["prompt"].as_str())
                {
                    excluded.push(prompt.to_string());
                }
                feedback = quality::repair_feedback(&error);
                let fatal = error
                    .downcast_ref::<super::provider::ProviderFailure>()
                    .is_some_and(|failure| !failure.retryable);
                let exhausted = if error.is::<DuplicateQuestion>() {
                    let used = duplicate_replaced;
                    duplicate_replaced = true;
                    used
                } else {
                    let used = content_repaired;
                    content_repaired = true;
                    used
                };
                if attempt == 3 || exhausted || fatal {
                    return Err(super::bounded_failure(error));
                }
            }
        }
    }
    unreachable!("three bounded draft attempts")
}

async fn exact_duplicates(
    context: &Preparation<'_>,
    prompts: &PromptSet,
    excluded: &mut Vec<String>,
) -> anyhow::Result<()> {
    let mut tx = interviewer_transaction(context.pool, context.claim.recruiter).await?;
    let incompatible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM interview_questions WHERE job_id=$1 AND (embedding_model<>$2 OR embedding_dimensions<>$3))")
        .bind(context.job).bind(&context.provider.embedding_model).bind(context.provider.dimensions as i32).fetch_one(&mut *tx).await?;
    anyhow::ensure!(
        !incompatible,
        "Existing question bank requires embedding migration before changing the embedding model"
    );
    let mut duplicated = false;
    for question in &prompts.questions {
        let collision: Option<String> = sqlx::query_scalar(
            "SELECT prompt FROM interview_questions WHERE job_id=$1 AND fingerprint=$2 LIMIT 1",
        )
        .bind(context.job)
        .bind(fingerprint(&question.prompt))
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(collision) = collision {
            duplicated = true;
            if !excluded.contains(&collision) {
                excluded.push(collision);
            }
        }
    }
    tx.commit().await?;
    if duplicated {
        return Err(DuplicateQuestion.into());
    }
    Ok(())
}
async fn semantic_duplicates(
    context: &Preparation<'_>,
    prompts: &PromptSet,
    vectors: &[Vec<f32>],
    excluded: &mut Vec<String>,
) -> anyhow::Result<()> {
    let mut tx = interviewer_transaction(context.pool, context.claim.recruiter).await?;
    let mut duplicated = false;
    for (question, embedding) in prompts.questions.iter().zip(vectors) {
        let collision=sqlx::query("SELECT prompt FROM interview_questions WHERE job_id=$1 AND (fingerprint=$2 OR (embedding_model=$3 AND embedding_dimensions=$4 AND 1-(embedding <=> $5::vector)>=$6)) ORDER BY id LIMIT 1")
            .bind(context.job).bind(fingerprint(&question.prompt)).bind(&context.provider.embedding_model).bind(context.provider.dimensions as i32).bind(vector_text(embedding)?).bind(context.provider.similarity).fetch_optional(&mut *tx).await?;
        if let Some(collision) = collision {
            duplicated = true;
            let text: String = collision.get("prompt");
            if !excluded.contains(&text) {
                excluded.push(text);
            }
        }
    }
    tx.commit().await?;
    if duplicated {
        return Err(DuplicateQuestion.into());
    }
    Ok(())
}
