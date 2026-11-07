//! Invitation validation, assignment, encryption and atomic link recovery.
use crate::{
    ApiError, Result,
    audit::record as audit,
    digest,
    error::{bad, conflict, missing},
    infrastructure::interviewer_transaction,
    invitations::InvitationCipher,
    models::invitation::{CreatedInvitation, RecoveredInvitation},
    new_token,
    repositories::invitation_repository::{self as repo, NewInvitation},
};
use axum::http::StatusCode;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) struct InviteCandidate {
    pub candidate_name: String,
    pub candidate_email: String,
    pub duration_minutes: i32,
    pub request_key: Uuid,
}
pub(crate) async fn invite(
    pool: &PgPool,
    user: Uuid,
    job: Uuid,
    input: InviteCandidate,
) -> Result<CreatedInvitation> {
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
    let mut tx = interviewer_transaction(pool, user).await?;
    let recruiter = user;
    repo::job(&mut tx, recruiter, job)
        .await?
        .ok_or_else(missing)?;
    repo::insert_interviewee(&mut tx, &email).await?;
    let interviewee = repo::interviewee(&mut tx, &email).await?.ok_or(conflict(
        "This email belongs to an interviewer. Use the interviewee's email address.",
    ))?;
    let id = Uuid::new_v4();
    let inserted = repo::insert(
        &mut tx,
        NewInvitation {
            id,
            owner: recruiter,
            job,
            request_key: input.request_key,
            name,
            email: &email,
            duration: input.duration_minutes,
            interviewee,
        },
    )
    .await?;
    if inserted && !crate::generation::bank::assign(&mut tx, recruiter, id).await? {
        return Err(conflict(
            "No completed question set is available for this job and interview duration. Complete question preparation before inviting a candidate.",
        ));
    }
    let result = repo::by_request(&mut tx, recruiter, input.request_key).await?;
    if result.job_id != job
        || result.candidate_name != name
        || result.candidate_email != email
        || result.duration_minutes != input.duration_minutes
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
        repo::insert_token(&mut tx, &digest(&token), id, &sealed).await?;
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
        let session_id = result.id;
        let sealed = repo::sealed_token(&mut tx, session_id, recruiter).await?;
        sealed
            .map(|value| {
                cipher()?
                    .open(&value, recruiter, session_id)
                    .map_err(|_| link_unavailable())
            })
            .transpose()?
    };
    tx.commit().await?;
    Ok(CreatedInvitation {
        session: result,
        token,
        created: inserted,
    })
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
pub(crate) async fn retrieve_invitation(
    pool: &PgPool,
    user: Uuid,
    id: Uuid,
) -> Result<RecoveredInvitation> {
    let mut tx = interviewer_transaction(pool, user).await?;
    let recruiter = user;
    let session = repo::lock_session(&mut tx, id, recruiter)
        .await?
        .ok_or_else(missing)?;
    if !session.active || !matches!(session.state.as_str(), "invited" | "ready" | "in_progress") {
        return Err(conflict(
            "Invitation links are unavailable for expired or submitted sessions",
        ));
    }
    let row = repo::lock_token(&mut tx, id, recruiter)
        .await?
        .ok_or_else(missing)?;
    let sealed = row.sealed_token;
    let replaced_legacy_link = sealed.is_none();
    // Historical hashes cannot be reversed. Explicit recovery reissues legacy links
    // atomically; new invitations always return the exact original token.
    let token = if let Some(sealed) = sealed {
        let token = cipher()?
            .open(&sealed, recruiter, id)
            .map_err(|_| link_unavailable())?;
        if digest(&token) != row.token_hash {
            return Err(link_unavailable());
        }
        token
    } else {
        if session.state != "invited" {
            return Err(conflict(
                "This legacy session has already been accepted; its original link cannot be recovered",
            ));
        }
        let token = new_token();
        let sealed = cipher()?
            .seal(&token, recruiter, id)
            .map_err(|_| link_unavailable())?;
        repo::replace_token(&mut tx, &digest(&token), &sealed, id, recruiter).await?;
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
    let expires_at = row.expires_at;
    tx.commit().await?;
    Ok(RecoveredInvitation {
        token,
        expires_at,
        replaced_legacy_link,
    })
}
