//! Cheap prompt validation and bank deduplication happen before reference generation.
use super::{
    pipeline::Preparation,
    quality,
    types::{PromptSet, cosine, fingerprint, vector_text},
};
use crate::infrastructure::interviewer_transaction;
use serde_json::json;
use sqlx::Row;

pub(super) async fn prepare(
    context: &Preparation<'_>,
    mut excluded: Vec<String>,
    initial_feedback: &str,
) -> anyhow::Result<(PromptSet, Vec<Vec<f32>>)> {
    let mut feedback = initial_feedback.to_string();
    let mut previous = None;
    for attempt in 1..=2 {
        tracing::info!(event = "task.phase", phase = "questions.generate", attempt);
        let response = context.provider.structured::<PromptSet>("interview_question_prompts",quality::PROMPT_INSTRUCTIONS,
            json!({"job_description":context.description,"blueprint":context.blueprint,"seed":context.claim.seed,"attempt":context.claim.attempts,"draft_attempt":attempt,"duration_minutes":context.duration,"excluded_questions":excluded,"repair_feedback":feedback,"previous_draft":previous})).await;
        let checked = async {
            let prompts = response?;
            previous = Some(serde_json::to_value(&prompts)?);
            tracing::info!(event = "task.phase", phase = "questions.validate");
            prompts.validate(context.blueprint, context.duration)?;
            exact_duplicates(context, &prompts, &mut excluded).await?;
            tracing::info!(event = "task.phase", phase = "questions.deduplicate");
            let texts: Vec<String> = prompts
                .questions
                .iter()
                .map(|question| question.prompt.clone())
                .collect();
            let vectors = context.provider.embeddings(&texts).await?;
            for (index, vector) in vectors.iter().enumerate() {
                for other in &vectors[..index] {
                    anyhow::ensure!(
                        cosine(vector, other) < context.provider.similarity,
                        "Generated set contains semantically duplicate questions"
                    );
                }
            }
            semantic_duplicates(context, &prompts, &vectors, &mut excluded).await?;
            tracing::info!(event = "task.phase", phase = "questions.review");
            quality::review_prompts(
                context.provider,
                context.description,
                context.blueprint,
                &prompts,
                context.duration,
            )
            .await?;
            Ok::<_, anyhow::Error>((prompts, vectors))
        }
        .await;
        match checked {
            Ok(approved) => return Ok(approved),
            Err(error) => {
                let fatal = error
                    .downcast_ref::<super::provider::ProviderFailure>()
                    .is_some_and(|failure| !failure.retryable);
                feedback = quality::repair_feedback(&error);
                if attempt == 2 || fatal {
                    return Err(super::bounded_failure(error));
                }
            }
        }
    }
    unreachable!("both prompt attempts return or repair")
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
    anyhow::ensure!(
        !duplicated,
        "Question duplicated an existing JD-bank question; choose a different task from the exclusion list"
    );
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
    anyhow::ensure!(
        !duplicated,
        "Question duplicated an existing JD-bank question; choose a different task from the exclusion list"
    );
    Ok(())
}
