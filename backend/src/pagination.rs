//! Bounded keyset pagination shared by list endpoints.
use crate::{Result, error::bad};
use serde::Deserialize;
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
