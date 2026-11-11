//! Session HTTP route composition. Feature handlers live in sibling files.
use crate::controllers::candidate_controller::{candidate_session, candidate_transition};
use crate::controllers::interview_controller::{list_sessions, session_detail, summary};
use crate::controllers::invitation_controller::{invite, retrieve_invitation};
use axum::{
    Router,
    routing::{delete, get, post},
};
use sqlx::PgPool;
pub fn routes() -> Router<PgPool> {
    Router::new()
        .route(
            "/api/v1/sessions/{session}/live-screen",
            post(crate::controllers::live_screen_controller::staff),
        )
        .route(
            "/api/v1/candidate/live-screen",
            post(crate::controllers::live_screen_controller::candidate),
        )
        .route("/api/v1/summary", get(summary))
        .route(
            "/api/v1/jobs",
            get(crate::controllers::job_controller::list_jobs)
                .post(crate::controllers::job_controller::create_job),
        )
        .route(
            "/api/v1/jobs/{job}",
            delete(crate::controllers::job_controller::delete_job),
        )
        .route(
            "/api/v1/jobs/{job}/blueprint",
            post(crate::controllers::job_controller::retry_blueprint),
        )
        .route("/api/v1/jobs/{job}/invitations", post(invite))
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/sessions/{session}", get(session_detail))
        .route(
            "/api/v1/sessions/{session}/invitation",
            post(retrieve_invitation),
        )
        .route(
            "/api/v1/audit",
            get(crate::controllers::audit_controller::audit_log),
        )
        .route("/api/v1/candidate/session", get(candidate_session))
        .route(
            "/api/v1/candidate/session/{action}",
            post(candidate_transition),
        )
}
