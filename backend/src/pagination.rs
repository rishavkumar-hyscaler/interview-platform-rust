//! Bounded keyset pagination shared by list endpoints.
use crate::{Result, error::bad};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
#[derive(Deserialize)]
pub(crate) struct Page {
    pub(crate) cursor: Option<Uuid>,
    pub(crate) limit: Option<i64>,
    pub(crate) entity_id: Option<Uuid>,
}
impl Page {
    pub(crate) fn limit(&self) -> Result<i64> {
        match self.limit.unwrap_or(20) {
            n @ 1..=100 => Ok(n),
            _ => Err(bad("Limit must be between 1 and 100")),
        }
    }
}
pub(crate) fn paginated(mut items: Vec<Value>, limit: i64) -> Value {
    let more = items.len() > limit as usize;
    items.truncate(limit as usize);
    let cursor = if more {
        items.last().and_then(|item| item.get("id")).cloned()
    } else {
        None
    };
    json!({"items":items,"next_cursor":cursor})
}
