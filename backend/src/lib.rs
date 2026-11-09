//! Layered API: controllers, services, repositories, internal models and explicit DTOs.
#[cfg(all(feature = "tokio-console", not(tokio_unstable)))]
compile_error!(
    "Tokio Console requires RUSTFLAGS=\"--cfg tokio_unstable\"; use the documented console startup command."
);
mod access;
mod ai;
mod app;
pub(crate) use repositories::audit_repository as audit;
mod controllers;
mod dto;
mod error;
pub use services::generation;
pub mod infrastructure;
pub(crate) use controllers::interview_routes as interviews;
pub mod invitations;
pub(crate) use services::media_service as media;
mod models;
mod observability;
#[cfg(debug_assertions)]
mod openapi;
mod pagination;
pub(crate) use services::proctoring_service as proctoring;
pub use services::rag;
mod repositories;
pub(crate) use services::review;
pub use services::scoring;
mod services;
pub mod workers;
pub use app::{router, router_with_ai};
pub(crate) use controllers::auth_controller::authenticate;
pub use error::ApiError;
pub(crate) use error::Result;
pub(crate) use services::auth_service::{digest, new_token, unauthorized};
pub use workers as worker;
