//! Allocate a private question set for each candidate; never reuse a published set.
use crate::repositories::generation_workflow_repository as repo;
use serde_json::json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub(crate) async fn assign(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    session: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = repo::assignment_candidate(tx, session).await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let job: Uuid = row.job_id;
    let duration: i32 = row.duration_minutes;
    repo::lock_job(tx, job).await?;
    let id = Uuid::new_v4();
    repo::insert_set(tx, id, owner, job, duration, Uuid::new_v4()).await?;
    repo::assign_set(tx, session, id).await?;
    repo::record_assignment(
        tx,
        Uuid::new_v4(),
        owner,
        session,
        json!({"set_id":id,"source":"candidate_preparation","reused":false}),
    )
    .await?;
    Ok(true)
}
