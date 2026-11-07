//! Stable evidence IDs and bounded UTF-8 chunks; no provider calls here.
use crate::repositories::evidence_index_repository as repo;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub(super) use crate::models::rag::EvidenceChunk as Chunk;
fn add(
    chunks: &mut Vec<Chunk>,
    id: &str,
    kind: &'static str,
    text: &str,
    session: Option<Uuid>,
    question: Option<Uuid>,
) {
    let chars: Vec<char> = text.chars().collect();
    let mut start = 0;
    let mut index = 0;
    while start < chars.len() {
        let end = (start + 1800).min(chars.len());
        let text: String = chars[start..end].iter().collect();
        let hash = hex::encode(Sha256::digest(text.as_bytes()));
        chunks.push(Chunk {
            source_id: format!("{id}:{kind}:{index}"),
            kind,
            text,
            hash,
            session,
            question,
        });
        if end == chars.len() {
            break;
        }
        start = end - 200;
        index += 1;
    }
}
// Render string values without JSON escaping so quotes can be copied verbatim.
fn readable(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(values) => values
            .iter()
            .enumerate()
            .map(|(i, v)| format!("Item {}:\n{}", i + 1, readable(v)))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| format!("{key}: {}", readable(value)))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => value.to_string(),
    }
}
pub(super) async fn load(
    tx: &mut Transaction<'_, Postgres>,
    session: Uuid,
) -> anyhow::Result<Vec<Chunk>> {
    let rows: Vec<Value> = repo::questions(tx, session).await?;
    let mut chunks = Vec::new();
    for row in rows {
        let id = row["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing question ID"))?;
        let question = id.parse::<Uuid>()?;
        add(
            &mut chunks,
            id,
            "prompt",
            row["prompt"].as_str().unwrap_or(""),
            None,
            Some(question),
        );
        add(
            &mut chunks,
            id,
            "rubric",
            &readable(&row["rubric"]),
            None,
            Some(question),
        );
        let answer = row["answer"].as_str().unwrap_or("");
        let text = format!(
            "Question: {}\nCandidate answer: {}",
            row["prompt"].as_str().unwrap_or(""),
            if answer.trim().is_empty() {
                "No written answer submitted."
            } else {
                answer
            }
        );
        add(&mut chunks, id, "answer", &text, Some(session), None);
    }
    let assessment: Option<(Uuid, Value)> = repo::assessment(tx, session).await?;
    if let Some((id, result)) = assessment {
        add(
            &mut chunks,
            &id.to_string(),
            "assessment",
            &readable(&result),
            Some(session),
            None,
        );
    }
    let events: Vec<(Uuid, Value)> = repo::events(tx, session).await?;
    for (id, event) in events {
        add(
            &mut chunks,
            &id.to_string(),
            "event",
            &readable(&event),
            Some(session),
            None,
        );
    }
    Ok(chunks)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunks_are_stable_overlapping_and_unicode_safe() {
        let text = "界🦀 ".repeat(1000);
        let mut first = Vec::new();
        let mut second = Vec::new();
        add(&mut first, "a", "answer", &text, Some(Uuid::nil()), None);
        add(&mut second, "a", "answer", &text, Some(Uuid::nil()), None);
        assert_eq!(first[0].hash, second[0].hash);
        assert!(
            first
                .iter()
                .all(|chunk| chunk.text.chars().count() <= 1800)
        );
        assert_eq!(
            first[0]
                .text
                .chars()
                .skip(1600)
                .collect::<String>(),
            first[1]
                .text
                .chars()
                .take(200)
                .collect::<String>()
        );
        assert_eq!(
            readable(&serde_json::json!({"answer":"a\nb"})),
            "answer: a\nb"
        );
    }
}
