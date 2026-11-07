//! HTTP errors. Never expose database messages or rejected input.
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
#[derive(Debug)]
pub struct ApiError(pub(crate) StatusCode, pub(crate) &'static str);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let code = match self.0 {
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => "invalid_request",
            StatusCode::UNAUTHORIZED => "unauthorized",
            StatusCode::FORBIDDEN => "forbidden",
            StatusCode::NOT_FOUND => "not_found",
            StatusCode::CONFLICT => "conflict",
            _ => "internal_error",
        };
        (self.0, Json(json!({"error": self.1, "code": code}))).into_response()
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(
            database_code = error
                .as_database_error()
                .and_then(|e| e.code())
                .as_deref(),
            "database operation failed"
        );
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Request could not be completed",
        )
    }
}
pub(crate) type Result<T> = std::result::Result<T, ApiError>;
pub(crate) fn bad(message: &'static str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message)
}
pub(crate) fn missing() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "Resource not found")
}
pub(crate) fn conflict(message: &'static str) -> ApiError {
    ApiError(StatusCode::CONFLICT, message)
}
