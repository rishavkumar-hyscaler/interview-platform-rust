//! Bounded concurrent reference generation; repair only the failing reference.
use super::{
    pipeline::Preparation,
    provider::AiProvider,
    quality::{self, QuestionReview, Review},
    types::{Blueprint, PromptSet, Question, QuestionPrompt, QuestionSet, ReferenceAnswer},
};
use serde_json::json;
use std::sync::Arc;
use tokio::{sync::Semaphore, task::JoinSet};

pub(super) async fn prepare(
    context: &Preparation<'_>,
    prompts: PromptSet,
) -> anyhow::Result<(QuestionSet, Review)> {
    let concurrency = match std::env::var("GENERATION_REFERENCE_CONCURRENCY") {
        Ok(value) => value.parse::<usize>().unwrap_or(3).clamp(1, 6),
        Err(_) => 3,
    };
    tracing::info!(
        event = "task.phase",
        phase = "references.generate_and_review",
        concurrency
    );
    let permits = Arc::new(Semaphore::new(concurrency));
    let count = prompts.questions.len();
    let mut tasks = JoinSet::new();
    let span = tracing::Span::current();
    for (index, prompt) in prompts.questions.into_iter().enumerate() {
        let provider = context.provider.clone();
        let description = context.description.to_string();
        let mut blueprint = context.blueprint.clone();
        blueprint.slots = vec![blueprint.slots[index].clone()];
        let permits = permits.clone();
        let span = tracing::info_span!(parent: &span,"reference_question",position=index+1);
        tasks.spawn(async move {
            use tracing::Instrument;
            async move {
                let _permit = permits.acquire_owned().await?;
                let (question, mut review) =
                    prepare_one(&provider, &description, &blueprint, &prompt).await?;
                review.position = index + 1;
                Ok::<_, anyhow::Error>((index, question, review))
            }
            .instrument(span)
            .await
        });
    }
    let mut completed = Vec::with_capacity(count);
    while let Some(result) = tasks.join_next().await {
        // Dropping JoinSet cancels remaining in-flight requests if this set fails.
        completed.push(result.map_err(|_| anyhow::anyhow!("Reference task interrupted"))??);
    }
    completed.sort_by_key(|(index, _, _)| *index);
    let mut questions = Vec::with_capacity(count);
    let mut reviews = Vec::with_capacity(count);
    for (_, question, review) in completed {
        questions.push(question);
        reviews.push(review);
    }
    let questions = QuestionSet { questions };
    questions.validate(context.blueprint)?;
    questions.validate_budget(context.duration)?;
    let review = Review { questions: reviews };
    review.validate(count)?;
    Ok((questions, review))
}

async fn prepare_one(
    provider: &AiProvider,
    jd: &str,
    blueprint: &Blueprint,
    prompt: &QuestionPrompt,
) -> anyhow::Result<(Question, QuestionReview)> {
    let mut feedback = String::new();
    let mut previous = None;
    for attempt in 1..=2 {
        let reference = provider.structured::<ReferenceAnswer>("interview_reference_answer",quality::REFERENCE_INSTRUCTIONS,json!({"job_description":jd,"blueprint":blueprint,"approved_question":prompt,"reference_attempt":attempt,"repair_feedback":feedback,"previous_reference":previous})).await;
        let checked = async {
            let reference = reference?;
            previous = Some(serde_json::to_value(&reference)?);
            let question = reference.attach(prompt);
            let questions = QuestionSet {
                questions: vec![question],
            };
            questions.validate(blueprint)?;
            // The prompt's allocation is immutable and already passed total-budget review.
            let mut review = quality::review(
                provider,
                jd,
                blueprint,
                &questions,
                prompt.estimated_minutes as i32,
            )
            .await?;
            let question = questions
                .questions
                .into_iter()
                .next()
                .expect("one approved question");
            Ok::<_, anyhow::Error>((question, review.questions.remove(0)))
        }
        .await;
        match checked {
            Ok(reference) => return Ok(reference),
            Err(error) => {
                let fatal = error
                    .downcast_ref::<super::provider::ProviderFailure>()
                    .is_some_and(|failure| !failure.retryable);
                feedback = quality::repair_feedback(&error);
                tracing::info!(event = "task.phase", phase = "reference.repair", attempt);
                if attempt == 2 || fatal {
                    return Err(super::bounded_failure(error));
                }
            }
        }
    }
    unreachable!("both reference attempts return or repair")
}
