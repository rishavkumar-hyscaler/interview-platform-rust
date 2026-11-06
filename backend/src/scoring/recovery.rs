//! Retry checkpoints and safe operational failure messages. Never log answer text.
use crate::generation::provider::{ProviderFailure, TransportFailure};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) fn fingerprint(question: &Value, model: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(
            json!({"version":"rubric-v1","model":model,"input":question})
                .to_string()
                .as_bytes()
        )
    )
}
#[derive(Debug)]
pub(super) struct QuestionFailure {
    pub position: u64,
    pub reason: String,
}
impl std::fmt::Display for QuestionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Question {}: {}", self.position, self.reason)
    }
}
impl std::error::Error for QuestionFailure {}
pub(super) fn safe_reason(error: &anyhow::Error) -> String {
    if let Some(provider) = error.downcast_ref::<ProviderFailure>() {
        return provider.to_string();
    }
    if let Some(transport) = error.downcast_ref::<TransportFailure>() {
        return transport.to_string();
    }
    let message = error.to_string();
    match message.as_str() {
        "Missing rubric"
        | "Incomplete criteria"
        | "Unknown criterion"
        | "Invalid points"
        | "Invalid rationale"
        | "Evidence must quote the answer exactly"
        | "AI service returned invalid JSON"
        | "AI service response exceeded size limit"
        | "AI response was incomplete or refused"
        | "AI response has no output"
        | "AI message has no content"
        | "AI refused to generate questions"
        | "AI output text is invalid"
        | "AI output did not match the required schema" => message,
        _ => "Scoring request or validation failed (unclassified)".into(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoint_hash_changes_with_answer_rubric_and_model() {
        let original = json!({"answer":"one","rubric":{"max_score":2}});
        let first = fingerprint(&original, "model-a");
        assert_eq!(first, fingerprint(&original, "model-a"));
        assert_ne!(first, fingerprint(&original, "model-b"));
        let mut changed = original.clone();
        changed["answer"] = json!("two");
        assert_ne!(first, fingerprint(&changed, "model-a"));
        changed = original;
        changed["rubric"]["max_score"] = json!(3);
        assert_ne!(first, fingerprint(&changed, "model-a"));
    }
    #[test]
    fn failures_never_echo_arbitrary_text() {
        assert_eq!(
            safe_reason(&anyhow::anyhow!("Evidence must quote the answer exactly")),
            "Evidence must quote the answer exactly"
        );
        assert!(!safe_reason(&anyhow::anyhow!("secret answer and API key")).contains("secret"));
    }
}
