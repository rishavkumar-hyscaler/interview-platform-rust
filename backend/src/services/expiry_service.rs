//! Durable session expiry orchestration. The caller controls transaction boundaries.
use crate::infrastructure::interviewer_transaction;
use crate::repositories::expiry_repository as repo;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
/// Short transactional jobs: crashes roll back work and acknowledgement together.
/// Concurrent workers skip locked sessions; finalization and audit commit together.
pub async fn process_expiry_job(pool: &PgPool) -> Result<bool, sqlx::Error> {
    let interviewers: Vec<Uuid> = repo::interviewers(pool).await?;
    for interviewer in interviewers {
        let mut tx = interviewer_transaction(pool, interviewer).await?;
        let session: Option<Uuid> = repo::next_due_session(&mut tx).await?;
        if let Some(session) = session {
            let mut task = crate::observability::TaskLog::start("session.expire", session);
            finalize_due_session(&mut tx, interviewer, session).await?;
            tx.commit().await?;
            task.finish(true);
            return Ok(true);
        }
        tx.commit().await?;
    }
    Ok(false)
}

/// Finalize once under a row lock; late answer writes cannot race submission.
pub async fn finalize_due_session(
    tx: &mut Transaction<'_, Postgres>,
    interviewer: Uuid,
    session: Uuid,
) -> Result<(), sqlx::Error> {
    repo::lock_session(tx, session).await?;
    let row = repo::finalize(tx, session).await?;
    if let Some(state) = row {
        repo::record_expiry(
            tx,
            Uuid::new_v4(),
            interviewer,
            session,
            if state == "submitted" {
                "session.timed_out"
            } else {
                "session.expired"
            },
        )
        .await?;
    }
    Ok(())
}
