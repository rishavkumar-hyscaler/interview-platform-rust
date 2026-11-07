//! Candidate session lifecycle. Authentication supplies the scoped transaction.
use crate::{
    Result, audit::record as audit, error::conflict, models::candidate::SessionSnapshot,
    repositories::candidate_repository as repo,
};
use serde_json::json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;
pub(crate) async fn session(
    owner: Uuid,
    id: Uuid,
    mut tx: Transaction<'_, Postgres>,
) -> Result<SessionSnapshot> {
    crate::infrastructure::finalize_due_session(&mut tx, owner, id).await?;
    crate::generation::bank::assign(&mut tx, owner, id).await?;
    let result = repo::view(&mut tx, id).await?;
    tx.commit().await?;
    Ok(result)
}
fn next_state(current: &str, action: &str) -> Result<&'static str> {
    match (current, action) {
        ("invited" | "ready", "ready") => Ok("ready"),
        ("ready" | "in_progress", "start") => Ok("in_progress"),
        ("in_progress" | "submitted", "submit") => Ok("submitted"),
        _ => Err(conflict("This session transition is not allowed")),
    }
}
pub(crate) async fn transition(
    recruiter: Uuid,
    id: Uuid,
    mut tx: Transaction<'_, Postgres>,
    action: String,
) -> Result<SessionSnapshot> {
    crate::infrastructure::finalize_due_session(&mut tx, recruiter, id).await?;
    let row = repo::lock_state(&mut tx, id).await?;
    let current = row.state;
    let next = next_state(&current, &action)?;
    if next != current {
        if !row.active {
            return Err(conflict("Session has expired"));
        }
        if action == "start" {
            let count = repo::question_count(&mut tx, id).await?;
            let prepared = row.prepared;
            if count == 0 || !prepared {
                return Err(conflict("Questions are not prepared yet"));
            }
        }
        repo::transition(&mut tx, id, next).await?;
        audit(
            &mut tx,
            recruiter,
            None,
            id,
            &format!("session.{next}"),
            json!({"actor":"candidate","previous_state":current}),
        )
        .await?;
    }
    if action == "ready" {
        crate::generation::bank::assign(&mut tx, recruiter, id).await?;
    }
    let result = repo::view(&mut tx, id).await?;
    tx.commit().await?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transitions_are_ordered_and_retries_are_safe() {
        assert_eq!(next_state("invited", "ready").unwrap(), "ready");
        assert_eq!(next_state("ready", "ready").unwrap(), "ready");
        assert_eq!(next_state("ready", "start").unwrap(), "in_progress");
        assert_eq!(next_state("in_progress", "start").unwrap(), "in_progress");
        assert_eq!(next_state("in_progress", "submit").unwrap(), "submitted");
        assert_eq!(next_state("submitted", "submit").unwrap(), "submitted");
        for (state, action) in [
            ("invited", "submit"),
            ("invited", "start"),
            ("submitted", "start"),
            ("expired", "ready"),
            ("completed", "submit"),
            ("ready", "completed"),
        ] {
            assert!(next_state(state, action).is_err());
        }
    }
}
