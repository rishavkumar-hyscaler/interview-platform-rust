//! In-process live scoring feed. The worker publishes the rationale while the model
//! writes it; reviewers subscribe over SSE. Nothing here is persisted or authoritative:
//! only the validated `assessment.question.scored` audit record is a score.
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
use uuid::Uuid;

struct Channel {
    sender: broadcast::Sender<Value>,
    /// Latest snapshot, so a reviewer who connects mid-question sees the text so far.
    latest: Option<Value>,
}
static HUB: LazyLock<Mutex<HashMap<Uuid, Channel>>> = LazyLock::new(Default::default);

fn with_channel<R>(session: Uuid, f: impl FnOnce(&mut Channel) -> R) -> R {
    let mut hub = HUB
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let channel = hub
        .entry(session)
        .or_insert_with(|| Channel {
            sender: broadcast::channel(64).0,
            latest: None,
        });
    f(channel)
}

/// Subscribe to a session's live scoring events, starting with the current snapshot.
pub(crate) fn subscribe(session: Uuid) -> (Option<Value>, broadcast::Receiver<Value>) {
    with_channel(session, |channel| {
        (channel.latest.clone(), channel.sender.subscribe())
    })
}

/// Drop the channel once nobody listens and no attempt is running.
pub(crate) fn release(session: Uuid) {
    let mut hub = HUB
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if hub
        .get(&session)
        .is_some_and(|channel| channel.latest.is_none() && channel.sender.receiver_count() == 0)
    {
        hub.remove(&session);
    }
}

fn publish(session: Uuid, event: Value, keep: bool) {
    with_channel(session, |channel| {
        channel.latest = keep.then(|| event.clone());
        // No receivers is normal; the worker never waits for reviewers.
        let _ = channel.sender.send(event);
    });
}

/// Publishes throttled rationale snapshots for one question being graded.
pub(crate) struct QuestionFeed {
    session: Uuid,
    position: u64,
    text: String,
    last: Option<Instant>,
}
impl QuestionFeed {
    pub(crate) fn start(session: Uuid, position: u64) -> Self {
        let mut feed = Self {
            session,
            position,
            text: String::new(),
            last: None,
        };
        feed.emit();
        feed
    }
    pub(crate) fn push(&mut self, delta: &str) {
        self.text.push_str(delta);
        if self
            .last
            .is_none_or(|last| last.elapsed() >= Duration::from_millis(250))
        {
            self.emit();
        }
    }
    fn emit(&mut self) {
        self.last = Some(Instant::now());
        let event = json!({"type":"rationale","position":self.position,"criteria":progress(&self.text),"verified":false});
        publish(self.session, event, true);
    }
    /// The grade passed validation and was saved; reviewers should reload it.
    pub(crate) fn scored(self, result: &Value) {
        publish(
            self.session,
            json!({"type":"scored","position":self.position,"result":result,"verified":true}),
            false,
        );
    }
    /// The streamed draft was rejected, so it must not be shown as a rationale.
    pub(crate) fn rejected(self) {
        publish(
            self.session,
            json!({"type":"rejected","position":self.position}),
            false,
        );
    }
}
/// The attempt ended; the stored assessment or failure is now authoritative.
pub(crate) fn finished(session: Uuid, success: bool) {
    publish(session, json!({"type":"finished","success":success}), false);
    release(session);
}

/// Extract per-criterion fields from a partial `{"criteria":[{...}]}` stream.
/// Unterminated strings are returned as far as they have been written.
pub(crate) fn progress(partial: &str) -> Vec<Value> {
    let mut criteria: Vec<Value> = Vec::new();
    let mut depth = 0usize;
    let mut key: Option<String> = None;
    let mut expecting_value = false;
    let mut chars = partial.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                depth += 1;
                if depth == 2 {
                    criteria.push(json!({}));
                }
                expecting_value = false;
            }
            '}' => depth = depth.saturating_sub(1),
            ':' => expecting_value = true,
            ',' | '[' | ']' => expecting_value = false,
            '"' => {
                let mut text = String::new();
                while let Some(next) = chars.next() {
                    match next {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => text.push('\n'),
                            Some('t') => text.push('\t'),
                            Some('r') => {}
                            Some('u') => {
                                let hex: String = chars.by_ref().take(4).collect();
                                if let Some(ch) = u32::from_str_radix(&hex, 16)
                                    .ok()
                                    .and_then(char::from_u32)
                                {
                                    text.push(ch);
                                }
                            }
                            Some(other) => text.push(other),
                            None => {}
                        },
                        other => text.push(other),
                    }
                }
                if expecting_value {
                    if depth == 2
                        && let (Some(name), Some(item)) = (key.take(), criteria.last_mut())
                    {
                        item[name] = json!(text);
                    }
                    expecting_value = false;
                } else {
                    key = Some(text);
                }
            }
            '0'..='9' if expecting_value => {
                let mut digits = c.to_string();
                while let Some(next) = chars
                    .peek()
                    .filter(|d| d.is_ascii_digit())
                {
                    digits.push(*next);
                    chars.next();
                }
                // A number at the very end may still be growing.
                if depth == 2
                    && chars.peek().is_some()
                    && let (Some(name), Some(item)) = (key.take(), criteria.last_mut())
                {
                    item[name] = json!(digits.parse::<u64>().unwrap_or(0));
                }
                expecting_value = false;
            }
            _ => {}
        }
    }
    criteria
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_rationale_is_readable_before_the_json_closes() {
        let partial = r#"{"criteria":[{"criterion_id":"ownership","reasoning":"Explains \"move\" semantics\nclearly","evidence":"moves","points":3},{"criterion_id":"lifetimes","reasoning":"Starts to discu"#;
        let items = progress(partial);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["criterion_id"], "ownership");
        assert_eq!(
            items[0]["reasoning"],
            "Explains \"move\" semantics\nclearly"
        );
        assert_eq!(items[0]["points"], 3);
        assert_eq!(items[1]["reasoning"], "Starts to discu");
        assert!(items[1].get("points").is_none());
    }
    #[test]
    fn trailing_number_is_not_reported_until_complete() {
        assert!(
            progress(r#"{"criteria":[{"points":1"#)[0]
                .get("points")
                .is_none()
        );
        assert_eq!(progress(r#"{"criteria":[{"points":10}"#)[0]["points"], 10);
    }
    #[test]
    fn late_subscribers_receive_the_latest_snapshot() {
        let session = Uuid::new_v4();
        let mut feed = QuestionFeed::start(session, 2);
        feed.push(r#"{"criteria":[{"criterion_id":"a","reasoning":"Partial"#);
        let (snapshot, _receiver) = subscribe(session);
        assert_eq!(snapshot.unwrap()["position"], 2);
        feed.scored(&json!({"score":1}));
        assert!(subscribe(session).0.is_none());
        finished(session, true);
    }
}
