//! Comparisons of stored competency scores need no model interpretation or API call.
use super::citations::{Citation, Document, Reply};
use serde_json::Value;
use uuid::Uuid;

pub(crate) fn assessment_source(session: Uuid, assessment: Option<&(Uuid, Value)>) -> Document {
    match assessment {
        Some((id, result)) => {
            let mut lines = vec!["Latest completed competency assessment:".to_string()];
            if let Some(competencies) = result["competency_profile"]["competencies"].as_array() {
                for competency in competencies {
                    let name = competency["name"]
                        .as_str()
                        .unwrap_or("Unnamed competency");
                    match competency["percent"]
                        .as_f64()
                        .filter(|p| p.is_finite() && (0.0..=100.0).contains(p))
                    {
                        Some(percent) => lines.push(format!("{name}: {percent:.2}%")),
                        None => lines.push(format!("{name}: not scored")),
                    }
                }
            }
            if lines.len() == 1 {
                lines.push("No comparable competency scores were recorded.".into());
            }
            Document {
                id: format!("{id}:competency-scores"),
                text: lines.join("\n"),
            }
        }
        None => Document {
            id: format!("{session}:assessment-status"),
            text: "No completed scoring assessment is recorded for this session.".into(),
        },
    }
}
pub(crate) fn comparison(
    question: &str,
    assessment: Option<&(Uuid, Value)>,
    source: &Document,
) -> Option<Reply> {
    let question = question.to_lowercase();
    // Do not mistake 'best practices' or a question about one answer for an area ranking.
    let area = ["area", "competenc", "skill", "topic"]
        .iter()
        .any(|term| question.contains(term));
    let best = ["best", "strongest", "highest"]
        .iter()
        .any(|term| question.contains(term));
    let worst = ["worst", "weakest", "lowest"]
        .iter()
        .any(|term| question.contains(term));
    if !area || best == worst || question.contains("best practice") {
        return None;
    }
    let scores: Vec<_> = assessment
        .and_then(|(_, a)| a["competency_profile"]["competencies"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let score = c["percent"].as_f64()?;
            if !score.is_finite() || !(0.0..=100.0).contains(&score) {
                return None;
            }
            Some((
                c["name"]
                    .as_str()
                    .unwrap_or("Unnamed competency"),
                score,
            ))
        })
        .collect();
    let target = scores
        .iter()
        .map(|(_, score)| *score)
        .reduce(|a, b| if best { a.max(b) } else { a.min(b) });
    let (answer, quotes) = match target {
        Some(target) => {
            let leaders: Vec<_> = scores.iter().filter(|(_, score)| (*score - target).abs() < 0.000000001)
                .map(|(name, score)| format!("{name}: {score:.2}%")).collect();
            let answer = format!("Based on the saved rubric scores, the {} scored area{} {} {}. This compares scored competencies only; unscored areas are excluded. Human review is still required.", if best { "highest" } else { "lowest" }, if leaders.len() > 1 { "s" } else { "" }, if leaders.len() > 1 { "are" } else { "is" }, leaders.join("; "));
            (answer, leaders)
        }
        None => ("There are no completed, comparable competency scores yet, so I cannot identify the candidate’s strongest or weakest area.".into(), vec![source.text.clone()]),
    };
    Some(Reply {
        answer,
        citations: quotes
            .into_iter()
            .map(|quote| Citation {
                source_id: source.id.clone(),
                quote,
            })
            .collect(),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn comparisons_use_saved_percentages_handle_ties_and_exclude_unscored_areas() {
        let assessment = (
            Uuid::new_v4(),
            json!({"competency_profile":{"competencies":[{"name":"React","percent":80},{"name":"Rust","percent":80},{"name":"SQL","percent":40},{"name":"Git","percent":null}]}}),
        );
        let source = assessment_source(Uuid::new_v4(), Some(&assessment));
        let mut best = comparison(
            "in which area the candidate did best",
            Some(&assessment),
            &source,
        )
        .unwrap();
        assert!(
            best.answer
                .contains("React: 80.00%; Rust: 80.00%")
        );
        assert_eq!(best.citations.len(), 2);
        super::super::citations::ground_reply(&mut best, &[source.clone()]).unwrap();
        let worst = comparison("which skill was weakest?", Some(&assessment), &source).unwrap();
        assert!(worst.answer.contains("SQL: 40.00%"));
        assert!(
            comparison(
                "which skills follow best practices?",
                Some(&assessment),
                &source
            )
            .is_none()
        );
    }
    #[test]
    fn missing_assessment_does_not_guess_from_reference_answers() {
        let source = assessment_source(Uuid::new_v4(), None);
        let mut reply = comparison("which area was best?", None, &source).unwrap();
        assert!(reply.answer.contains("cannot identify"));
        super::super::citations::ground_reply(&mut reply, &[source]).unwrap();
    }
}
