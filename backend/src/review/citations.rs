//! Resolve short model-facing references and preserve exact source quotes.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Document {
    pub id: String,
    pub text: String,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Citation {
    pub source_id: String,
    pub quote: String,
}
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Reply {
    pub answer: String,
    pub citations: Vec<Citation>,
}
// Track byte ranges so whitespace matching returns the actual source substring.
fn normalized(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut value = String::new();
    let mut spans = Vec::new();
    let mut space: Option<(usize, usize)> = None;
    for (start, c) in text.char_indices() {
        let end = start + c.len_utf8();
        if c.is_whitespace() {
            if !value.is_empty() {
                space = Some((space.map_or(start, |s| s.0), end));
            }
        } else {
            if let Some(span) = space.take() {
                value.push(' ');
                spans.push(span);
            }
            value.push(c);
            spans.extend(std::iter::repeat_n((start, end), c.len_utf8()));
        }
    }
    (value, spans)
}
fn exact_quote(source: &str, quote: &str) -> Option<String> {
    if quote.trim().is_empty() {
        return None;
    }
    if source.contains(quote) {
        return Some(quote.to_string());
    }
    let (text, spans) = normalized(source);
    let (quote, _) = normalized(quote);
    let start = text.find(&quote)?;
    let end = start + quote.len() - 1;
    Some(source[spans[start].0..spans[end].1].to_string())
}
pub(super) fn ground_reply(reply: &mut Reply, docs: &[Document]) -> anyhow::Result<()> {
    anyhow::ensure!(
        !reply.answer.trim().is_empty()
            && reply.answer.len() <= 12000
            && (1..=12).contains(&reply.citations.len()),
        "Invalid reply"
    );
    for citation in &mut reply.citations {
        let doc = docs
            .iter()
            .enumerate()
            .find(|(index, doc)| {
                citation.source_id == doc.id || citation.source_id == format!("S{}", index + 1)
            })
            .map(|(_, doc)| doc)
            .ok_or_else(|| anyhow::anyhow!("Unknown citation source"))?;
        citation.quote = exact_quote(&doc.text, &citation.quote)
            .ok_or_else(|| anyhow::anyhow!("Citation quote not found"))?;
        citation.source_id = doc.id.clone();
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whitespace_and_short_ids_resolve_to_original_source_bytes() {
        let docs = vec![Document {
            id: "session:answer:0".into(),
            text: "React: 80%\n\n  Réactivité ✅".into(),
        }];
        let mut reply = Reply {
            answer: "Recorded result".into(),
            citations: vec![Citation {
                source_id: "S1".into(),
                quote: "React: 80% Réactivité ✅".into(),
            }],
        };
        ground_reply(&mut reply, &docs).unwrap();
        assert_eq!(reply.citations[0].source_id, docs[0].id);
        assert_eq!(reply.citations[0].quote, docs[0].text);
    }
    #[test]
    fn invented_numbers_wording_sources_and_empty_citations_are_rejected() {
        let docs = vec![Document {
            id: "a".into(),
            text: "React: 80%".into(),
        }];
        for (id, quote) in [
            ("S1", "React: 90%"),
            ("S1", "React is excellent"),
            ("other-session", "React: 80%"),
            ("S0", "React: 80%"),
            ("S1", " "),
        ] {
            let mut reply = Reply {
                answer: "Summary".into(),
                citations: vec![Citation {
                    source_id: id.into(),
                    quote: quote.into(),
                }],
            };
            assert!(ground_reply(&mut reply, &docs).is_err());
        }
        assert!(
            ground_reply(
                &mut Reply {
                    answer: "Invented claim".into(),
                    citations: vec![]
                },
                &docs
            )
            .is_err()
        );
    }
}
