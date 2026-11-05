//! Session HTTP route composition. Feature handlers live in sibling files.
mod candidate;
mod invitations;
mod staff;
use axum::{
    Router,
    routing::{get, post},
};
pub(crate) use candidate::candidate_view;
use candidate::{candidate_session, candidate_transition};
use invitations::{invite, retrieve_invitation};
use sqlx::PgPool;
use staff::{list_sessions, session_detail, summary};
pub fn routes() -> Router<PgPool> {
    Router::new()
        .route("/api/v1/summary", get(summary))
        .route(
            "/api/v1/jobs",
            get(crate::jobs::list_jobs).post(crate::jobs::create_job),
        )
        .route("/api/v1/jobs/{job}/invitations", post(invite))
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/sessions/{session}", get(session_detail))
        .route(
            "/api/v1/sessions/{session}/invitation",
            post(retrieve_invitation),
        )
        .route("/api/v1/audit", get(crate::audit::audit_log))
        .route("/api/v1/candidate/session", get(candidate_session))
        .route(
            "/api/v1/candidate/session/{action}",
            post(candidate_transition),
        )
}
