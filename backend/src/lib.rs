pub mod infrastructure;
mod interviews;
pub mod invitations;
pub mod worker;
use crate::infrastructure::tenant_transaction;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rand::{RngCore, rngs::OsRng};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

#[derive(Debug)]
pub struct ApiError(StatusCode, &'static str);
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
        tracing::error!(%error, "database operation failed");
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Request could not be completed",
        )
    }
}
type Result<T> = std::result::Result<T, ApiError>;
fn unauthorized() -> ApiError {
    ApiError(StatusCode::UNAUTHORIZED, "Invalid or expired credentials")
}

pub fn router(pool: PgPool) -> Router {
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(json!({"status": "ok"})) }),
        )
        .route("/health/ready", get(ready))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/me", get(me))
        .route("/api/organizations/{id}", get(organization))
        .merge(interviews::routes())
        .layer(
            tower_http::set_header::SetResponseHeaderLayer::if_not_present(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            ),
        )
        .layer(DefaultBodyLimit::max(128 * 1024))
        .layer(axum::middleware::map_response(normalize_errors))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(pool)
}
// Normalize extractor/routing failures without echoing rejected input or secrets.
async fn normalize_errors(response: Response) -> Response {
    if response.status().is_client_error()
        && !response
            .headers()
            .get("content-type")
            .is_some_and(|v| v.as_bytes().starts_with(b"application/json"))
    {
        return (response.status(), Json(json!({"error":"Invalid request, route, or request body", "code":"invalid_request"}))).into_response();
    }
    response
}
async fn ready(State(pool): State<PgPool>) -> Result<Json<Value>> {
    sqlx::query("SELECT 1").execute(&pool).await?;
    Ok(Json(json!({"status": "ready"})))
}
#[derive(Deserialize)]
pub struct Credentials {
    email: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    email: String,
    password: String,
}
fn validate(email: &str, password: &str) -> Result<()> {
    if email.len() > 254
        || !email.contains('@')
        || email.chars().any(char::is_whitespace)
        || !(12..=128).contains(&password.len())
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Provide an email and a password of 12–128 bytes",
        ));
    }
    Ok(())
}
fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
async fn register(
    State(pool): State<PgPool>,
    Json(input): Json<Registration>,
) -> Result<(StatusCode, Json<Value>)> {
    if std::env::var("ALLOW_REGISTRATION").as_deref() != Ok("true") {
        return Err(ApiError(StatusCode::FORBIDDEN, "Registration is disabled"));
    }
    let email = input.email.trim().to_lowercase();
    validate(&email, &input.password)?;
    let hash = tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(input.password.as_bytes(), &SaltString::generate(&mut OsRng))
            .map(|h| h.to_string())
    })
    .await
    .map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Authentication unavailable",
        )
    })?
    .map_err(|_| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Authentication unavailable",
        )
    })?;
    let user = Uuid::new_v4();
    let org = Uuid::new_v4();
    let token = new_token();
    let mut tx = tenant_transaction(&pool, org).await?;
    let inserted = sqlx::query(
        "INSERT INTO users(id,email,password_hash,interview_scope_id) VALUES ($1,$2,$3,$4) ON CONFLICT(email) DO NOTHING",
    )
    .bind(user)
    .bind(&email)
    .bind(hash)
    .bind(org)
    .execute(&mut *tx)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Account could not be created",
        ));
    }
    sqlx::query("INSERT INTO organizations(id,name) VALUES ($1,$2)")
        .bind(org)
        .bind("Interview records")
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships(organization_id,user_id,role) VALUES ($1,$2,'recruiter')")
        .bind(org)
        .bind(user)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO auth_sessions(token_hash,user_id) VALUES ($1,$2)")
        .bind(digest(&token))
        .bind(user)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO audit_events(id,organization_id,actor_id,action) VALUES ($1,$2,$3,'recruiter.registered')")
        .bind(Uuid::new_v4()).bind(org).bind(user).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"token":token,"user_id":user,"organization_id":org,"role":"recruiter"})),
    ))
}
async fn login(State(pool): State<PgPool>, Json(input): Json<Credentials>) -> Result<Json<Value>> {
    let email = input.email.trim().to_lowercase();
    validate(&email, &input.password).map_err(|_| unauthorized())?;
    let row = sqlx::query("SELECT id,password_hash,interview_scope_id FROM users WHERE email=$1")
        .bind(email)
        .fetch_optional(&pool)
        .await?;
    // Always run Argon2, including for unknown accounts, to reduce account timing leakage.
    let hash = row.as_ref().map(|r| r.get::<String, _>("password_hash"));
    let valid = tokio::task::spawn_blocking(move || match hash {
        Some(hash) => PasswordHash::new(&hash).is_ok_and(|h| {
            Argon2::default()
                .verify_password(input.password.as_bytes(), &h)
                .is_ok()
        }),
        None => {
            let _ = Argon2::default()
                .hash_password(input.password.as_bytes(), &SaltString::generate(&mut OsRng));
            false
        }
    })
    .await
    .map_err(|_| unauthorized())?;
    if !valid {
        return Err(unauthorized());
    }
    let row = row.ok_or_else(unauthorized)?;
    let user: Uuid = row.get("id");
    let org: Uuid = row.get("interview_scope_id");
    let token = new_token();
    sqlx::query("INSERT INTO auth_sessions(token_hash,user_id) VALUES ($1,$2)")
        .bind(digest(&token))
        .bind(user)
        .execute(&pool)
        .await?;
    Ok(Json(
        json!({"token":token,"user_id":user,"organization_id":org,"role":"recruiter"}),
    ))
}
async fn authenticate(pool: &PgPool, headers: &HeaderMap) -> Result<(Uuid, String)> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| v.len() == 64)
        .ok_or_else(unauthorized)?;
    let hash = digest(token);
    let user = sqlx::query_scalar(
        "SELECT user_id FROM auth_sessions WHERE token_hash=$1 AND expires_at>now()",
    )
    .bind(&hash)
    .fetch_optional(pool)
    .await?
    .ok_or_else(unauthorized)?;
    Ok((user, hash))
}
async fn me(State(pool): State<PgPool>, headers: HeaderMap) -> Result<Json<Value>> {
    let (user, _) = authenticate(&pool, &headers).await?;
    let org: Uuid = sqlx::query_scalar("SELECT interview_scope_id FROM users WHERE id=$1")
        .bind(user)
        .fetch_one(&pool)
        .await?;
    Ok(Json(
        json!({"user_id":user,"organization_id":org,"role":"recruiter"}),
    ))
}
async fn logout(State(pool): State<PgPool>, headers: HeaderMap) -> Result<StatusCode> {
    let (_, hash) = authenticate(&pool, &headers).await?;
    sqlx::query("DELETE FROM auth_sessions WHERE token_hash=$1")
        .bind(hash)
        .execute(&pool)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
async fn organization(
    State(pool): State<PgPool>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (user, _) = authenticate(&pool, &headers).await?;
    let mut tx = tenant_transaction(&pool, org).await?;
    let row = sqlx::query("SELECT o.id,o.name,m.role FROM organizations o JOIN memberships m ON m.organization_id=o.id WHERE o.id=$1 AND m.user_id=$2")
        .bind(org).bind(user).fetch_optional(&mut *tx).await?
        .ok_or(ApiError(StatusCode::NOT_FOUND, "Organization not found"))?;
    let result = json!({"id":row.get::<Uuid,_>("id"),"name":row.get::<String,_>("name"),"role":row.get::<String,_>("role")});
    tx.commit().await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_have_bounds() {
        assert!(validate("user@example.com", "long-enough-password").is_ok());
        assert!(validate("user", "long-enough-password").is_err());
        assert!(validate("user@example.com", "short").is_err());
        assert!(validate("user@example.com", &"x".repeat(129)).is_err());
    }
    #[test]
    fn tokens_are_random_and_hashed() {
        let a = new_token();
        let b = new_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
        assert_ne!(digest(&a), a);
    }
}
