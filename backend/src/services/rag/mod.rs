//! Persistent post-scoring retrieval; independent from the scoring outcome.
mod documents;
mod retrieval;
pub(crate) mod worker;
pub(crate) use crate::controllers::rag_controller::routes;
pub(crate) use retrieval::{fallback, search};
pub use worker::process_next;

#[cfg(test)]
mod tests;
