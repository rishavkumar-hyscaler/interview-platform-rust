//! Assessment HTTP interface, worker orchestration and rubric rules.
pub(crate) mod live;
mod recovery;
#[cfg(test)]
mod rig_tests;
mod rubric;
mod worker;
pub use crate::controllers::scoring_controller::routes;
pub(crate) use rubric::evaluate;
pub use worker::process_next;
