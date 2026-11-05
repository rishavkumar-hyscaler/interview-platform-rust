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
    #[schemars(length(min = 2, max = 120))]
    pub name: String,
    #[schemars(length(min = 10, max = 500))]
    pub jd_evidence: String,
    #[schemars(range(min = 1, max = 100))]
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
    #[schemars(length(min = 1, max = 40), regex(pattern = r"^[a-z0-9_]{1,40}$"))]
    pub id: String,
    #[schemars(length(min = 10, max = 1000))]
    pub description: String,
    #[schemars(range(min = 1, max = 10))]
    pub max_points: u32,
    #[schemars(length(min = 20, max = 1500))]
    pub partial_credit: String,
    #[schemars(length(min = 20, max = 1500))]
    pub full_credit: String,
    #[schemars(length(min = 30, max = 2000))]
    pub reference_answer: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub competency_id: String,
    pub kind: Kind,
    pub difficulty: Difficulty,
    #[schemars(length(min = 30, max = 4000))]
    pub prompt: String,
    #[schemars(length(min = 120, max = 12000))]
    pub expected_answer: String,
    #[schemars(range(min = 1, max = 30))]
    pub estimated_minutes: u32,
    #[schemars(length(max = 5), inner(length(min = 10, max = 1000)))]
    pub assumptions: Vec<String>,
    #[schemars(length(min = 1, max = 5), inner(length(min = 20, max = 1500)))]
    pub acceptable_alternatives: Vec<String>,
    #[schemars(length(min = 1, max = 5), inner(length(min = 20, max = 1000)))]
    pub common_mistakes: Vec<String>,
    pub coding: Option<CodeReference>,
    #[schemars(length(min = 2, max = 6))]
    pub criteria: Vec<Criterion>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestCategory {
    Typical,
    Edge,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferenceTest {
    pub category: TestCategory,
    #[schemars(length(min = 1, max = 2000))]
    pub input: String,
    #[schemars(length(min = 1, max = 2000))]
    pub expected_output: String,
    #[schemars(length(min = 10, max = 1000))]
    pub explanation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeReference {
    #[schemars(length(min = 1, max = 80))]
    pub language: String,
    #[schemars(length(min = 40, max = 12000))]
    pub solution: String,
    #[schemars(length(min = 20, max = 1000))]
    pub complexity: String,
    #[schemars(length(min = 2, max = 5))]
    pub tests: Vec<ReferenceTest>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionSet {
    pub questions: Vec<Question>,
}
/// Prompt-only contract: no answers or rubrics can be generated at this stage.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionPrompt {
    pub competency_id: String,
    pub kind: Kind,
    pub difficulty: Difficulty,
    #[schemars(length(min = 30, max = 4000))]
    pub prompt: String,
    #[schemars(range(min = 1, max = 30))]
    pub estimated_minutes: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PromptSet {
    pub questions: Vec<QuestionPrompt>,
}
impl PromptSet {
    pub fn validate(&self, blueprint: &Blueprint, duration: i32) -> Result<()> {
        ensure!(
            self.questions.len() == blueprint.slots.len(),
            "Generated question count does not match blueprint"
        );
        let mut fingerprints = HashSet::new();
        let mut total = 0u64;
        for (index, (question, slot)) in self.questions.iter().zip(&blueprint.slots).enumerate() {
            ensure!(
                question.competency_id == slot.competency_id
                    && question.kind == slot.kind
                    && question.difficulty == slot.difficulty,
                "Question {} does not match its blueprint slot",
                index + 1
            );
            ensure!(
                bounded(&question.prompt, 30, 4000),
                "Question {} prompt is invalid",
                index + 1
            );
            ensure!(
                (1..=30).contains(&question.estimated_minutes),
                "Question time budget is invalid"
            );
            ensure!(
                fingerprints.insert(fingerprint(&question.prompt)),
                "Generated set contains duplicate questions"
            );
            total += u64::from(question.estimated_minutes);
        }
        ensure!(
            duration > 0 && total <= duration as u64,
            "Question time budgets exceed the interview duration"
        );
        Ok(())
    }
}

/// The model authors only reference material; approved prompt fields stay immutable.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferenceAnswer {
    #[schemars(length(min = 120, max = 12000))]
    pub expected_answer: String,
    #[schemars(length(max = 5), inner(length(min = 10, max = 1000)))]
    pub assumptions: Vec<String>,
    #[schemars(length(min = 1, max = 5), inner(length(min = 20, max = 1500)))]
    pub acceptable_alternatives: Vec<String>,
    #[schemars(length(min = 1, max = 5), inner(length(min = 20, max = 1000)))]
    pub common_mistakes: Vec<String>,
    pub coding: Option<CodeReference>,
    #[schemars(length(min = 2, max = 6))]
    pub criteria: Vec<Criterion>,
}
impl ReferenceAnswer {
    pub fn attach(self, approved: &QuestionPrompt) -> Question {
        Question {
            competency_id: approved.competency_id.clone(),
            kind: approved.kind.clone(),
            difficulty: approved.difficulty.clone(),
            prompt: approved.prompt.clone(),
            estimated_minutes: approved.estimated_minutes,
            expected_answer: self.expected_answer,
            assumptions: self.assumptions,
            acceptable_alternatives: self.acceptable_alternatives,
            common_mistakes: self.common_mistakes,
            coding: self.coding,
            criteria: self.criteria,
        }
    }
}

fn bounded(value: &str, min: usize, max: usize) -> bool {
    !value.contains('\0') && (min..=max).contains(&value.trim().chars().count())
}
impl Blueprint {
    pub fn canonicalize_ids(&mut self) -> Result<()> {
        let mut mapping = std::collections::HashMap::new();
        let mut used = HashSet::new();
        for (index, competency) in self.competencies.iter().enumerate() {
            ensure!(
                !competency.id.trim().is_empty() && !mapping.contains_key(&competency.id),
                "Blueprint competency IDs must be nonempty and unique"
            );
            let raw: String = competency
                .id
                .chars()
                .flat_map(char::to_lowercase)
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                .collect();
            let mut id = raw
                .split('_')
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join("_");
            id.truncate(32);
            if id.is_empty() {
                id = format!("competency_{}", index + 1);
            }
            if !used.insert(id.clone()) {
                id = format!("{id}_{}", index + 1);
                ensure!(used.insert(id.clone()), "Blueprint competency IDs collide");
            }
            mapping.insert(competency.id.clone(), id);
        }
        let slots: Vec<String> = self
            .slots
            .iter()
            .map(|slot| {
                mapping.get(&slot.competency_id).cloned().ok_or_else(|| {
                    anyhow::anyhow!("Blueprint slot references an unknown competency")
                })
            })
            .collect::<Result<_>>()?;
        for competency in &mut self.competencies {
            competency.id = mapping[&competency.id].clone();
        }
        for (slot, id) in self.slots.iter_mut().zip(slots) {
            slot.competency_id = id;
        }
        Ok(())
    }
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
impl Blueprint {
    pub fn validate_priorities(&self) -> Result<()> {
        let mut names = HashSet::new();
        ensure!(
            self.competencies
                .iter()
                .all(|c| names.insert(fingerprint(&c.name))),
            "Blueprint competencies must be distinct"
        );
        let count = self.slots.len();
        let mut expected = vec![1usize; self.competencies.len()];
        for _ in self.competencies.len()..count {
            let index = (0..expected.len())
                .max_by(|a, b| {
                    let deficit = |i: usize| {
                        self.competencies[i].weight as i64 * count as i64 - expected[i] as i64 * 100
                    };
                    deficit(*a).cmp(&deficit(*b)).then_with(|| b.cmp(a))
                })
                .ok_or_else(|| anyhow::anyhow!("Blueprint has no competencies"))?;
            expected[index] += 1;
        }
        for (competency, expected) in self.competencies.iter().zip(expected) {
            let actual = self
                .slots
                .iter()
                .filter(|s| s.competency_id == competency.id)
                .count();
            ensure!(
                actual.abs_diff(expected) <= 1,
                "Blueprint slot allocation neglects JD competency priorities"
            );
        }
        Ok(())
    }
}
impl Question {
    pub fn worked_reference(&self) -> String {
        let mut answer = self.expected_answer.clone();
        for criterion in &self.criteria {
            answer.push_str(&format!(
                "\n\n[{}] {}\n{}",
                criterion.id, criterion.description, criterion.reference_answer
            ));
        }
        answer
    }
}
impl QuestionSet {
    pub fn validate(&self, blueprint: &Blueprint) -> Result<()> {
        self.validate_at(blueprint, 1)
    }
    pub(super) fn validate_at(&self, blueprint: &Blueprint, first_position: usize) -> Result<()> {
        ensure!(
            self.questions.len() == blueprint.slots.len(),
            "Generated question count does not match blueprint"
        );
        let mut prompts = HashSet::new();
        let mut failures = Vec::new();
        for (index, (q, slot)) in self.questions.iter().zip(&blueprint.slots).enumerate() {
            let checked = (|| -> Result<()> {
                ensure!(
                    q.competency_id == slot.competency_id
                        && q.kind == slot.kind
                        && q.difficulty == slot.difficulty,
                    "Question does not match its blueprint slot"
                );
                ensure!(
                    bounded(&q.prompt, 30, 4000) && bounded(&q.expected_answer, 120, 12000),
                    "Question or expected-answer guidance is invalid"
                );
                ensure!(
                    (1..=30).contains(&q.estimated_minutes),
                    "Question time budget is invalid"
                );
                ensure!(
                    q.assumptions.len() <= 5 && q.assumptions.iter().all(|s| bounded(s, 10, 1000)),
                    "Reference assumptions are invalid"
                );
                ensure!(
                    (1..=5).contains(&q.acceptable_alternatives.len())
                        && q.acceptable_alternatives
                            .iter()
                            .all(|s| bounded(s, 20, 1500)),
                    "Reference must describe acceptable alternative answers"
                );
                ensure!(
                    (1..=5).contains(&q.common_mistakes.len())
                        && q.common_mistakes.iter().all(|s| bounded(s, 20, 1000)),
                    "Reference must identify concrete mistakes"
                );
                match (&q.kind, &q.coding) {
                    (Kind::Code, Some(code)) => {
                        ensure!(
                            bounded(&code.language, 1, 80)
                                && bounded(&code.solution, 40, 12000)
                                && bounded(&code.complexity, 20, 1000),
                            "Coding reference is incomplete"
                        );
                        ensure!(
                            (2..=5).contains(&code.tests.len())
                                && code
                                    .tests
                                    .iter()
                                    .any(|t| t.category == TestCategory::Typical)
                                && code.tests.iter().any(|t| t.category == TestCategory::Edge),
                            "Coding reference needs typical and edge-case tests"
                        );
                        ensure!(
                            code.tests.iter().all(|t| bounded(&t.input, 1, 2000)
                                && bounded(&t.expected_output, 1, 2000)
                                && bounded(&t.explanation, 10, 1000)),
                            "Coding reference test is invalid"
                        );
                    }
                    (Kind::Code, None) => {
                        anyhow::bail!("Coding question needs a reference solution and tests")
                    }
                    (_, Some(_)) => {
                        anyhow::bail!("Non-coding question must not include a coding reference")
                    }
                    _ => {}
                }
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
                for (criterion_index, c) in q.criteria.iter().enumerate() {
                    let number = criterion_index + 1;
                    ensure!(
                        bounded(&c.id, 1, 40)
                            && c.id.bytes().all(|byte| byte.is_ascii_lowercase()
                                || byte.is_ascii_digit()
                                || byte == b'_'),
                        "Rubric criterion {number} ID must be 1–40 lowercase ASCII letters, digits or underscores"
                    );
                    ensure!(
                        ids.insert(&c.id),
                        "Rubric criterion {number} ID is duplicated"
                    );
                    ensure!(
                        bounded(&c.description, 10, 1000),
                        "Rubric criterion {number} description must be 10–1000 nonblank characters"
                    );
                    ensure!(
                        (1..=10).contains(&c.max_points),
                        "Rubric criterion {number} maximum points must be between 1 and 10"
                    );
                    ensure!(
                        bounded(&c.partial_credit, 20, 1500)
                            && bounded(&c.full_credit, 20, 1500)
                            && c.partial_credit.trim() != c.full_credit.trim(),
                        "Rubric needs distinct partial and full credit anchors"
                    );
                    ensure!(
                        bounded(&c.reference_answer, 30, 2000)
                            && c.reference_answer.trim() != c.description.trim()
                            && c.reference_answer.trim() != c.full_credit.trim()
                            && c.reference_answer.trim() != c.partial_credit.trim(),
                        "Every rubric criterion needs a substantive worked reference section"
                    );
                    points += u64::from(c.max_points);
                }
                ensure!(points == 10, "Rubric maximum points must total 10");
                Ok(())
            })();
            if let Err(error) = checked {
                failures.push(format!("Question {}: {}", first_position + index, error));
            }
        }
        ensure!(failures.is_empty(), "{}", failures.join("; "));
        Ok(())
    }
    pub fn validate_budget(&self, duration: i32) -> Result<()> {
        let total: u64 = self
            .questions
            .iter()
            .map(|q| u64::from(q.estimated_minutes))
            .sum();
        ensure!(
            duration > 0 && total <= duration as u64,
            "Question time budgets exceed the interview duration"
        );
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
    fn rubric_feedback_identifies_the_invalid_criterion_rule() {
        let (blueprint, mut set) = code_fixture();
        set.questions[0].criteria[0].id = "Correctness-Score".into();
        assert!(
            set.validate(&blueprint)
                .unwrap_err()
                .to_string()
                .contains("criterion 1 ID must")
        );
        let (_, mut set) = code_fixture();
        set.questions[0].criteria[1].id = set.questions[0].criteria[0].id.clone();
        assert!(
            set.validate(&blueprint)
                .unwrap_err()
                .to_string()
                .contains("criterion 2 ID is duplicated")
        );
        let (_, mut set) = code_fixture();
        set.questions[0].criteria[0].description = " ".repeat(20);
        assert!(
            set.validate(&blueprint)
                .unwrap_err()
                .to_string()
                .contains("description must")
        );
        let (_, mut set) = code_fixture();
        set.questions[0].criteria[0].max_points = 0;
        assert!(
            set.validate(&blueprint)
                .unwrap_err()
                .to_string()
                .contains("maximum points must")
        );
        let schema = serde_json::to_value(schemars::schema_for!(Criterion)).unwrap();
        assert_eq!(schema["properties"]["id"]["pattern"], "^[a-z0-9_]{1,40}$");
    }
    #[test]
    fn prompt_stage_rejects_answer_keys_and_reference_stage_cannot_replace_prompts() {
        let (blueprint, set) = code_fixture();
        let question = &set.questions[0];
        let prompt = QuestionPrompt {
            competency_id: question.competency_id.clone(),
            kind: question.kind.clone(),
            difficulty: question.difficulty.clone(),
            prompt: question.prompt.clone(),
            estimated_minutes: question.estimated_minutes,
        };
        PromptSet {
            questions: vec![prompt.clone()],
        }
        .validate(&blueprint, 3)
        .unwrap();
        let mut payload = serde_json::to_value(&prompt).unwrap();
        payload["expected_answer"] =
            serde_json::json!("An answer must not appear in the prompt contract");
        assert!(serde_json::from_value::<QuestionPrompt>(payload).is_err());
        let mut reference = serde_json::to_value(question).unwrap();
        for key in [
            "competency_id",
            "kind",
            "difficulty",
            "prompt",
            "estimated_minutes",
        ] {
            reference.as_object_mut().unwrap().remove(key);
        }
        let answer: ReferenceAnswer = serde_json::from_value(reference.clone()).unwrap();
        assert_eq!(answer.attach(&prompt).prompt, prompt.prompt);
        reference["prompt"] = serde_json::json!("Replace the approved question");
        assert!(serde_json::from_value::<ReferenceAnswer>(reference).is_err());
    }
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
    fn code_fixture() -> (Blueprint, QuestionSet) {
        let blueprint=serde_json::from_value(serde_json::json!({"competencies":[{"id":"api","name":"API correctness","jd_evidence":"Build correct APIs with Rust","weight":100}],"slots":[{"competency_id":"api","kind":"code","difficulty":"medium"}]})).unwrap();
        let questions=serde_json::from_value(serde_json::json!({"questions":[{"competency_id":"api","kind":"code","difficulty":"medium","prompt":"Implement add for two small signed integers. Return their sum; inputs are guaranteed not to overflow.","expected_answer":"Return the sum of the two input integers directly. The explicit no-overflow precondition makes ordinary signed addition sufficient. The function takes constant time and uses constant extra space. Zero and negative values follow the same addition rule.","estimated_minutes":3,"assumptions":["Use Rust because the job description specifies Rust."],"acceptable_alternatives":["Checked addition is acceptable if the documented no-overflow precondition is preserved."],"common_mistakes":["Concatenating string inputs instead of performing integer addition returns the wrong result."],"criteria":[{"id":"correctness","description":"Adds the integer inputs correctly.","max_points":7,"partial_credit":"Describes integer addition but does not provide a complete implementation.","full_credit":"Returns the correct integer sum for every permitted input.","reference_answer":"Return the sum of the two input integers directly."},{"id":"complexity","description":"Explains constant resource usage.","max_points":3,"partial_credit":"Identifies time cost but does not explain space usage.","full_credit":"Explains constant time and constant extra space.","reference_answer":"The function takes constant time and uses constant extra space."}],"coding":{"language":"Rust","solution":"fn add(a: i64, b: i64) -> i64 { a + b } // inputs cannot overflow","complexity":"The function uses O(1) time and O(1) extra space.","tests":[{"category":"typical","input":"add(2,3)","expected_output":"5","explanation":"Adds two positive integers."},{"category":"edge","input":"add(0,0)","expected_output":"0","explanation":"Handles the zero boundary."}]}}]})).unwrap();
        (blueprint, questions)
    }
    #[test]
    fn reference_rubric_alignment_and_code_examples_are_required() {
        let (blueprint, mut set) = code_fixture();
        blueprint
            .validate("Build correct APIs with Rust", 1)
            .unwrap();
        set.validate(&blueprint).unwrap();
        set.validate_budget(3).unwrap();
        assert!(set.validate_budget(2).is_err());
        set.questions[0].criteria[0].reference_answer = "Too short".into();
        assert!(set.validate(&blueprint).is_err());
        let (_, mut set) = code_fixture();
        set.questions[0].coding = None;
        assert!(set.validate(&blueprint).is_err());
        let (_, mut set) = code_fixture();
        set.questions[0].coding.as_mut().unwrap().tests[1].category = TestCategory::Typical;
        assert!(set.validate(&blueprint).is_err());
        let (_, mut set) = code_fixture();
        set.questions[0].criteria[0].partial_credit =
            set.questions[0].criteria[0].full_credit.clone();
        assert!(set.validate(&blueprint).is_err());
    }
    #[test]
    fn criterion_sections_are_compiled_into_the_reference_with_stable_ids() {
        let (blueprint, mut set) = code_fixture();
        let reference = "Compute the addition using the declared non-overflowing integer inputs and return the resulting integer.";
        set.questions[0].criteria[0].reference_answer = reference.into();
        set.validate(&blueprint).unwrap();
        assert!(!set.questions[0].expected_answer.contains(reference));
        let compiled = set.questions[0].worked_reference();
        assert!(compiled.contains(reference));
        assert!(compiled.contains("[correctness]"));
        assert!(compiled.contains("[complexity]"));
    }
    #[test]
    fn blueprint_identifier_normalization_preserves_slot_references() {
        let (mut blueprint, _) = code_fixture();
        blueprint.competencies[0].id = "Rust API / Transactions".into();
        blueprint.slots[0].competency_id = "Rust API / Transactions".into();
        blueprint.canonicalize_ids().unwrap();
        assert_eq!(blueprint.competencies[0].id, "rust_api_transactions");
        assert_eq!(blueprint.slots[0].competency_id, "rust_api_transactions");
        blueprint.slots[0].competency_id = "missing".into();
        assert!(blueprint.canonicalize_ids().is_err());
    }
}
