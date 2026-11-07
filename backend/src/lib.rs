//! One application, organized by feature; app.rs composes HTTP routes.
#[cfg(all(feature = "tokio-console", not(tokio_unstable)))]
compile_error!(
    "Tokio Console requires RUSTFLAGS=\"--cfg tokio_unstable\"; use the documented console startup command."
);
mod access;
mod adaptive;
mod ai;
mod app;
mod audit;
mod auth;
mod error;
pub mod generation;
pub mod infrastructure;
mod interview_room;
mod interviews;
pub mod invitations;
mod jobs;
mod media;
mod observability;
mod pagination;
mod proctor_evidence;
mod proctoring;
pub mod rag;
mod review;
pub mod schema;
pub mod scoring;
pub mod worker;
pub use app::{router, router_with_ai};
pub(crate) use auth::{authenticate, digest, new_token, unauthorized};
pub use error::ApiError;
pub(crate) use error::Result;
