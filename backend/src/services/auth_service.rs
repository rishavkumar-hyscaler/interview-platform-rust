//! Credential validation and authentication workflows; independent of HTTP DTOs.
use crate::{
    ApiError, Result, infrastructure::interviewer_transaction, models::auth::Session,
    repositories::auth_repository as repo,
};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use axum::http::StatusCode;
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) fn unauthorized() -> ApiError {
    ApiError(StatusCode::UNAUTHORIZED, "Invalid or expired credentials")
}

pub(crate) struct Credentials {
    pub email: String,
    pub password: String,
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
pub(crate) fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}
pub(crate) fn new_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
pub(crate) async fn register(pool: &PgPool, input: Credentials) -> Result<Session> {
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
    let token = new_token();
    let mut tx = interviewer_transaction(&pool, user).await?;
    if !repo::create_user(&mut tx, user, &email, &hash).await? {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "Account could not be created",
        ));
    }
    repo::insert_session(&mut tx, &digest(&token), user).await?;
    repo::record_registration(&mut tx, user).await?;
    tx.commit().await?;
    Ok(Session {
        token,
        user_id: user,
    })
}
pub(crate) async fn login(pool: &PgPool, input: Credentials) -> Result<Session> {
    let email = input.email.trim().to_lowercase();
    validate(&email, &input.password).map_err(|_| unauthorized())?;
    let row = repo::credentials(pool, &email).await?;
    // Always run Argon2, including for unknown accounts, to reduce account timing leakage.
    let hash = row.as_ref().map(|r| r.password_hash.clone());
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
    let user = row.id;
    let token = new_token();
    repo::login_session(pool, &digest(&token), user).await?;
    Ok(Session {
        token,
        user_id: user,
    })
}
pub(crate) async fn authenticate_token(pool: &PgPool, token: &str) -> Result<(Uuid, String)> {
    if token.len() != 64 {
        return Err(unauthorized());
    }
    let hash = digest(token);
    let user = repo::user_for_token(pool, &hash)
        .await?
        .ok_or_else(unauthorized)?;
    Ok((user, hash))
}
pub(crate) async fn logout(pool: &PgPool, hash: &str) -> Result<()> {
    repo::delete_session(pool, hash).await
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

/// Revalidate a long-running request before reading or writing its private context.
pub(crate) async fn require_session(pool: &PgPool, owner: Uuid, hash: &str) -> Result<()> {
    if repo::user_for_token(pool, hash).await? != Some(owner) {
        return Err(unauthorized());
    }
    Ok(())
}
