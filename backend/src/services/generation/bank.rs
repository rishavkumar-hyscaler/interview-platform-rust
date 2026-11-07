//! Prepare a fixed number of reusable job sets and link each session directly to a set.
use crate::repositories::generation_workflow_repository as repo;
use serde_json::json;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub(crate) async fn replenish(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    job: Uuid,
    duration: i32,
) -> Result<(), sqlx::Error> {
    // Serialize pool changes for a job; safe across multiple API/worker processes.
    let target: i32 = repo::target_count(tx, job).await?;
    let count: i64 = repo::set_count(tx, job).await?;
    for _ in count..i64::from(target) {
        repo::insert_set(tx, Uuid::new_v4(), owner, job, duration, Uuid::new_v4()).await?;
    }
    Ok(())
}

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
    // Serialize usage balancing so concurrent invitations choose the least-used set.
    repo::lock_job(tx, job).await?;
    let set = repo::select_assignment(tx, job, duration).await?;
    let Some(set) = set else {
        return Ok(false);
    };
    let id = set;
    repo::assign_set(tx, session, id).await?;
    repo::record_assignment(
        tx,
        Uuid::new_v4(),
        owner,
        session,
        json!({"set_id":id,"source":"job_pool","reused":true}),
    )
    .await?;
    Ok(true)
}
