//! Persistent post-scoring retrieval; independent from the scoring outcome.
mod documents;
mod http;
mod retrieval;
mod worker;
pub(crate) use http::routes;
pub(crate) use retrieval::{fallback, search};
pub use worker::process_next;

#[cfg(test)]
mod tests;
