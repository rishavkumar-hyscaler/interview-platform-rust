//! Structured operational logs: identifiers and outcomes, never request/answer content.
use axum::{
    extract::{MatchedPath, Request},
    middleware::Next,
    response::Response,
};
use std::time::Instant;
use tracing::Instrument;
use uuid::Uuid;

pub(crate) async fn request_log(request: Request, next: Next) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Uuid::parse_str(value).ok())
        .unwrap_or_else(Uuid::new_v4);
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str())
        .unwrap_or("unmatched")
        .to_owned();
    let method = request.method().clone();
    let span = tracing::info_span!("http_request", %request_id, %method, %route);
    async move {
        let started = Instant::now();
        tracing::info!(event = "request.started");
        let mut response = next.run(request).await;
        response.headers_mut().insert(
            "x-request-id",
            request_id
                .to_string()
                .parse()
                .expect("UUID header"),
        );
        let status = response.status().as_u16();
        // Duration ends at response headers; streams have separate task lifecycle logs.
        if status >= 500 {
            tracing::error!(
                event = "request.completed",
                status,
                duration_ms = started.elapsed().as_millis() as u64
            );
        } else {
            tracing::info!(
                event = "request.completed",
                status,
                duration_ms = started.elapsed().as_millis() as u64
            );
        }
        response
    }
    .instrument(span)
    .await
}

/// Logs cancellation too, including worker shutdown and disconnected streams.
pub(crate) struct TaskLog {
    operation: &'static str,
    task_id: Uuid,
    session_id: Uuid,
    started: Instant,
    finished: bool,
    parent: tracing::Span,
}
impl TaskLog {
    pub(crate) fn start(operation: &'static str, session_id: Uuid) -> Self {
        let value = Self {
            operation,
            task_id: Uuid::new_v4(),
            session_id,
            started: Instant::now(),
            finished: false,
            parent: tracing::Span::current(),
        };
        value.log("task.started", "running");
        value
    }
    pub(crate) fn span(&self) -> tracing::Span {
        tracing::info_span!("background_task", operation=self.operation, task_id=%self.task_id, session_id=%self.session_id)
    }
    fn log(&self, event: &'static str, outcome: &'static str) {
        let _guard = self.parent.enter();
        tracing::info!(event, operation=self.operation, task_id=%self.task_id, session_id=%self.session_id, outcome, duration_ms=self.started.elapsed().as_millis() as u64);
    }
    pub(crate) fn finish(&mut self, success: bool) {
        self.log(
            "task.completed",
            if success { "succeeded" } else { "failed" },
        );
        self.finished = true;
    }
}
impl Drop for TaskLog {
    fn drop(&mut self) {
        if !self.finished {
            self.log("task.interrupted", "cancelled_or_failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };
    use tower::ServiceExt;
    use tracing::instrument::WithSubscriber;

    #[derive(Clone)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .unwrap()
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn requests_are_correlated_without_logging_private_input() {
        let output = Capture(Arc::new(Mutex::new(Vec::new())));
        let writer = output.clone();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        async {
            let pool = sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://unused@localhost/unused")
                .unwrap();
            let router = crate::router_with_ai(pool, None);
            let id = Uuid::new_v4();
            let request = Request::builder()
                .uri("/health/live?token=private-query-secret")
                .header("x-request-id", id.to_string())
                .header("authorization", "Bearer private-auth-secret")
                .body(axum::body::Body::empty())
                .unwrap();
            let response = router
                .clone()
                .oneshot(request)
                .await
                .unwrap();
            assert_eq!(response.headers()["x-request-id"], id.to_string());
            assert_eq!(response.status(), 200);
            let request = Request::builder()
                .uri("/private-path-secret?token=other-secret")
                .header("x-request-id", "private-invalid-id")
                .body(axum::body::Body::empty())
                .unwrap();
            let response = router.oneshot(request).await.unwrap();
            assert!(
                Uuid::parse_str(
                    response.headers()["x-request-id"]
                        .to_str()
                        .unwrap()
                )
                .is_ok()
            );
            assert_eq!(response.status(), 404);
        }
        .with_subscriber(subscriber)
        .await;
        let logs = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
        assert!(logs.contains("request.completed"));
        assert!(logs.contains("/health/live"));
        assert!(logs.contains("unmatched"));
        for private in [
            "private-query-secret",
            "private-auth-secret",
            "private-path-secret",
            "private-invalid-id",
            "other-secret",
        ] {
            assert!(!logs.contains(private), "log leaked private input");
        }
    }
}
