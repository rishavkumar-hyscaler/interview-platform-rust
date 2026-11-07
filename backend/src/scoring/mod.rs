//! Assessment HTTP interface, worker orchestration and rubric rules.
mod http;
mod recovery;
#[cfg(test)]
mod rig_tests;
mod rubric;
mod worker;
pub use http::routes;
pub(crate) use rubric::evaluate;
pub use worker::process_next;
