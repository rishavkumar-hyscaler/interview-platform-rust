use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Text,
    Scenario,
    Code,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Easy,
    Medium,
    Hard,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Competency {
    pub id: String,
    pub name: String,
    pub jd_evidence: String,
    pub weight: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Slot {
    pub competency_id: String,
    pub kind: Kind,
    pub difficulty: Difficulty,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Blueprint {
    pub competencies: Vec<Competency>,
    pub slots: Vec<Slot>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Criterion {
    pub id: String,
    pub description: String,
    pub max_points: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub competency_id: String,
    pub kind: Kind,
    pub difficulty: Difficulty,
    pub prompt: String,
    pub expected_answer: String,
    pub criteria: Vec<Criterion>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionSet {
    pub questions: Vec<Question>,
}
fn bounded(value: &str, min: usize, max: usize) -> bool {
    !value.contains('\0') && (min..=max).contains(&value.trim().chars().count())
}
impl Blueprint {
    pub fn validate(&self, jd: &str, count: usize) -> Result<()> {
        ensure!(
            (1..=6).contains(&self.competencies.len()) && self.competencies.len() <= count,
            "Blueprint competency count is invalid"
        );
        ensure!(
            self.slots.len() == count,
            "Blueprint question count is invalid"
        );
        let mut ids = HashSet::new();
        let mut weight = 0u64;
        for c in &self.competencies {
            ensure!(
                bounded(&c.id, 1, 40)
                    && c.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    && ids.insert(c.id.clone()),
                "Blueprint competency IDs are invalid"
            );
            ensure!(
                bounded(&c.name, 2, 120)
                    && bounded(&c.jd_evidence, 10, 500)
                    && jd.contains(&c.jd_evidence),
                "Blueprint evidence must quote the job description exactly"
            );
            ensure!(
                (1..=100).contains(&c.weight),
                "Blueprint weights are invalid"
            );
            weight += u64::from(c.weight);
        }
        ensure!(weight == 100, "Blueprint weights must total 100");
        for slot in &self.slots {
            ensure!(
                ids.contains(&slot.competency_id),
                "Blueprint slot references an unknown competency"
            );
        }
        ensure!(
            ids.iter()
                .all(|id| self.slots.iter().any(|s| &s.competency_id == id)),
            "Blueprint must cover every competency"
        );
        let easy = self
            .slots
            .iter()
            .filter(|s| s.difficulty == Difficulty::Easy)
            .count();
        let hard = self
            .slots
            .iter()
            .filter(|s| s.difficulty == Difficulty::Hard)
            .count();
        ensure!(
            easy == count / 3 && hard == count / 3,
            "Blueprint must follow the configured difficulty mix"
        );
        Ok(())
    }
}
impl QuestionSet {
    pub fn validate(&self, blueprint: &Blueprint) -> Result<()> {
        ensure!(
            self.questions.len() == blueprint.slots.len(),
            "Generated question count does not match blueprint"
        );
        let mut prompts = HashSet::new();
        for (q, slot) in self.questions.iter().zip(&blueprint.slots) {
            ensure!(
                q.competency_id == slot.competency_id
                    && q.kind == slot.kind
                    && q.difficulty == slot.difficulty,
                "Question does not match its blueprint slot"
            );
            ensure!(
                bounded(&q.prompt, 30, 4000) && bounded(&q.expected_answer, 30, 6000),
                "Question or expected-answer guidance is invalid"
            );
            ensure!(
                prompts.insert(fingerprint(&q.prompt)),
                "Generated set contains duplicate questions"
            );
            ensure!(
                (2..=6).contains(&q.criteria.len()),
                "Rubric must contain 2–6 criteria"
            );
            let mut ids = HashSet::new();
            let mut points = 0u64;
            for c in &q.criteria {
                ensure!(
                    bounded(&c.id, 1, 40)
                        && ids.insert(&c.id)
                        && bounded(&c.description, 10, 1000)
                        && (1..=10).contains(&c.max_points),
                    "Rubric criteria are invalid"
                );
                points += u64::from(c.max_points);
            }
            ensure!(points == 10, "Rubric maximum points must total 10");
        }
        Ok(())
    }
}
pub fn fingerprint(prompt: &str) -> String {
    let normalized: String = prompt
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect();
    hex::encode(Sha256::digest(normalized.as_bytes()))
}
pub fn vector_text(values: &[f32]) -> Result<String> {
    ensure!(
        !values.is_empty()
            && values.len() <= 4096
            && values.iter().all(|x| x.is_finite())
            && values.iter().any(|x| *x != 0.0),
        "Embedding vector is invalid"
    );
    Ok(format!(
        "[{}]",
        values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    ))
}
pub fn cosine(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| f64::from(*x) * f64::from(*y))
        .sum();
    let norm = |v: &[f32]| v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    dot / (norm(a) * norm(b))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprints_ignore_formatting_and_vectors_are_checked() {
        assert_eq!(
            fingerprint("Explain a Rust API!"),
            fingerprint("EXPLAIN a Rust API?")
        );
        assert_ne!(
            fingerprint("Scale to 100 users"),
            fingerprint("Scale to 200 users")
        );
        assert!(vector_text(&[f32::NAN]).is_err());
        assert!(vector_text(&[0.0, 0.0]).is_err());
        assert!(vector_text(&[1.0, 0.0]).is_ok());
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
    }
}
