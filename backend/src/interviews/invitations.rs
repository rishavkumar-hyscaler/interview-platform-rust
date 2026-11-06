//! Create and recover encrypted candidate invitation links.
use crate::{
    ApiError, Result,
    access::staff,
    audit::record as audit,
    error::{bad, conflict, missing},
};
use crate::{digest, invitations::InvitationCipher, new_token};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use uuid::Uuid;
#[derive(Deserialize)]
pub(super) struct Invitation {
    candidate_name: String,
    candidate_email: String,
    duration_minutes: i32,
    request_key: Uuid,
}
pub(super) async fn invite(
    State(pool): State<PgPool>,
    Path(job): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<Invitation>,
) -> Result<(StatusCode, Json<Value>)> {
    let name = input.candidate_name.trim();
    let email = input.candidate_email.trim().to_lowercase();
    if name.contains('\0')
        || email.contains('\0')
        || !(1..=120).contains(&name.chars().count())
        || email.len() > 254
        || !email.contains('@')
        || email.chars().any(char::is_whitespace)
        || !(5..=180).contains(&input.duration_minutes)
    {
        return Err(bad(
            "Provide a candidate name, email, and a duration of 5–180 minutes",
        ));
    }
    let (user, mut tx) = staff(&pool, &headers).await?;
    let recruiter = user;
    let _job: Uuid = sqlx::query_scalar("SELECT id FROM jobs WHERE interviewer_id=$1 AND id=$2")
        .bind(recruiter)
        .bind(job)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(missing)?;
    sqlx::query("INSERT INTO users(id,email,role) VALUES($1,$2,'interviewee') ON CONFLICT(email,role) DO NOTHING")
        .bind(Uuid::new_v4()).bind(&email).execute(&mut *tx).await?;
    let interviewee: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email=$1 AND role='interviewee'")
            .bind(&email)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(conflict(
                "This email belongs to an interviewer. Use the interviewee's email address.",
            ))?;
    let id = Uuid::new_v4();
    let inserted = sqlx::query("INSERT INTO interview_sessions(id,interviewer_id,job_id,request_key,candidate_name,candidate_email,duration_minutes,interviewee_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(interviewer_id,request_key) DO NOTHING")
        .bind(id).bind(recruiter).bind(job).bind(input.request_key).bind(name).bind(&email).bind(input.duration_minutes).bind(interviewee).execute(&mut *tx).await?.rows_affected()==1;
    if inserted && !crate::generation::bank::assign(&mut tx, recruiter, id).await? {
        return Err(conflict(
            "No completed question set is available for this job and interview duration. Complete question preparation before inviting a candidate.",
        ));
    }
    let result: Value = sqlx::query_scalar("SELECT to_jsonb(s)-'request_key' FROM interview_sessions s WHERE interviewer_id=$1 AND request_key=$2")
        .bind(recruiter).bind(input.request_key).fetch_one(&mut *tx).await?;
    if result["job_id"] != job.to_string()
        || result["candidate_name"] != name
        || result["candidate_email"] != email
        || result["duration_minutes"] != input.duration_minutes
    {
        return Err(conflict(
            "Request key was already used with different invitation details",
        ));
    }
    let token = if inserted {
        let token = new_token();
        let sealed = cipher()?
            .seal(&token, recruiter, id)
            .map_err(|_| link_unavailable())?;
        sqlx::query("INSERT INTO auth_sessions(token_hash,user_id,interviewer_id,interview_session_id,expires_at,sealed_token) SELECT $1,interviewee_id,interviewer_id,id,expires_at,$3 FROM interview_sessions WHERE id=$2")
            .bind(digest(&token)).bind(id).bind(sealed).execute(&mut *tx).await?;
        audit(
            &mut tx,
            recruiter,
            Some(user),
            id,
            "session.invited",
            json!({"job_id":job}),
        )
        .await?;
        Some(token)
    } else {
        let session_id: Uuid = result["id"]
            .as_str()
            .ok_or_else(link_unavailable)?
            .parse()
            .map_err(|_| link_unavailable())?;
        let sealed: Option<String> = sqlx::query_scalar(
            "SELECT sealed_token FROM auth_sessions WHERE interview_session_id=$1 AND interviewer_id=$2",
        )
        .bind(session_id)
        .bind(recruiter)
        .fetch_one(&mut *tx)
        .await?;
        sealed
            .map(|value| {
                cipher()?
                    .open(&value, recruiter, session_id)
                    .map_err(|_| link_unavailable())
            })
            .transpose()?
    };
    tx.commit().await?;
    Ok((
        if inserted {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(json!({"session":result,"invitation_token":token})),
    ))
}
fn link_unavailable() -> ApiError {
    ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Invitation link could not be recovered. Check the configured encryption key.",
    )
}
fn cipher() -> Result<InvitationCipher> {
    InvitationCipher::from_environment().map_err(|_| link_unavailable())
}
pub(super) async fn retrieve_invitation(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (user, mut tx) = staff(&pool, &headers).await?;
    let recruiter = user;
    let session = sqlx::query("SELECT state, expires_at>now() AND least(expires_at,coalesce(deadline_at,expires_at))>now() AS active FROM interview_sessions WHERE id=$1 AND interviewer_id=$2 FOR UPDATE")
        .bind(id).bind(recruiter).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    if !session.get::<bool, _>("active")
        || !matches!(
            session.get::<String, _>("state").as_str(),
            "invited" | "ready" | "in_progress"
        )
    {
        return Err(conflict(
            "Invitation links are unavailable for expired or submitted sessions",
        ));
    }
    let row = sqlx::query("SELECT token_hash,sealed_token,expires_at::text AS expires_at FROM auth_sessions WHERE interview_session_id=$1 AND interviewer_id=$2 AND expires_at>now() FOR UPDATE")
        .bind(id).bind(recruiter).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    let sealed: Option<String> = row.get("sealed_token");
    let replaced_legacy_link = sealed.is_none();
    // Historical hashes cannot be reversed. Explicit recovery reissues legacy links
    // atomically; new invitations always return the exact original token.
    let token = if let Some(sealed) = sealed {
        let token = cipher()?
            .open(&sealed, recruiter, id)
            .map_err(|_| link_unavailable())?;
        if digest(&token) != row.get::<String, _>("token_hash") {
            return Err(link_unavailable());
        }
        token
    } else {
        if session.get::<String, _>("state") != "invited" {
            return Err(conflict(
                "This legacy session has already been accepted; its original link cannot be recovered",
            ));
        }
        let token = new_token();
        let sealed = cipher()?
            .seal(&token, recruiter, id)
            .map_err(|_| link_unavailable())?;
        sqlx::query("UPDATE auth_sessions SET token_hash=$1,sealed_token=$2 WHERE interview_session_id=$3 AND interviewer_id=$4")
            .bind(digest(&token)).bind(sealed).bind(id).bind(recruiter).execute(&mut *tx).await?;
        token
    };
    audit(
        &mut tx,
        recruiter,
        Some(user),
        id,
        if replaced_legacy_link {
            "invitation.link_reissued"
        } else {
            "invitation.link_retrieved"
        },
        json!({"replaced_legacy_link":replaced_legacy_link}),
    )
    .await?;
    let expires_at: String = row.get("expires_at");
    tx.commit().await?;
    Ok(Json(
        json!({"invitation_token":token,"expires_at":expires_at,"replaced_legacy_link":replaced_legacy_link}),
    ))
}
