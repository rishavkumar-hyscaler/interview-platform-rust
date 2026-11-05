//! Complete and review the reference for the current question before saving it.
use super::{
    pipeline::Preparation,
    provider::AiProvider,
    quality::{self, QuestionReview, Review},
    types::{Blueprint, PromptSet, Question, QuestionPrompt, QuestionSet, ReferenceAnswer},
};
use serde_json::json;

pub(super) async fn prepare(
    context: &Preparation<'_>,
    prompts: PromptSet,
) -> anyhow::Result<(QuestionSet, Review)> {
    anyhow::ensure!(
        prompts.questions.len() == 1,
        "Reference preparation requires one question"
    );
    let prompt = prompts
        .questions
        .into_iter()
        .next()
        .expect("one approved prompt");
    let diagnostics = super::diagnostics::Scope {
        pool: context.pool.clone(),
        owner: context.claim.recruiter,
        entity: context.claim.session,
        position: Some(context.position),
    };
    tracing::info!(
        event = "task.phase",
        phase = "reference.generate_and_review",
        position = context.position
    );
    let (question, review) = prepare_one(
        context.provider,
        context.description,
        context.blueprint,
        &prompt,
        &diagnostics,
    )
    .await?;
    Ok((
        QuestionSet {
            questions: vec![question],
        },
        Review {
            questions: vec![review],
        },
    ))
}

async fn prepare_one(
    provider: &AiProvider,
    jd: &str,
    blueprint: &Blueprint,
    prompt: &QuestionPrompt,
    diagnostics: &super::diagnostics::Scope,
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
            questions.validate_at(blueprint, diagnostics.position.unwrap_or(1))?;
            // The prompt's allocation is immutable and already passed total-budget review.
            let mut review = quality::review(
                provider,
                jd,
                blueprint,
                &questions,
                prompt.estimated_minutes as i32,
                diagnostics.position.unwrap_or(1),
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
                let snapshot = previous
                    .as_ref()
                    .map(|reference| json!({"approved_question":prompt,"reference":reference}));
                diagnostics
                    .rejected("reference", attempt, &error, &snapshot)
                    .await;
                if quality::prompt_defect(&error) {
                    return Err(quality::PromptRevision {
                        summary: format!(
                            "Question {} prompt needs revision: {}",
                            diagnostics.position.unwrap_or(1),
                            error
                        ),
                        feedback: quality::repair_feedback(&error),
                    }
                    .into());
                }
                let fatal = error
                    .downcast_ref::<super::provider::ProviderFailure>()
                    .is_some_and(|failure| !failure.retryable);
                feedback = quality::repair_feedback(&error);
                tracing::info!(event = "task.phase", phase = "reference.repair", attempt);
                if attempt == 2 && !fatal && quality::review_findings(&error).is_some() {
                    return Err(quality::PromptRevision {
                        summary: format!(
                            "Question {} reference still failed content review after repair: {}",
                            diagnostics.position.unwrap_or(1),
                            error
                        ),
                        feedback: quality::repair_feedback(&error),
                    }
                    .into());
                }
                if attempt == 2 || fatal {
                    return Err(super::bounded_failure(error));
                }
            }
        }
    }
    unreachable!("both reference attempts return or repair")
}
