//! Maintain a small unused pool and reserve each prepared set for one session only.
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

pub(crate) const POOL_SIZE: i64 = 3;

pub(crate) async fn replenish(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    job: Uuid,
    duration: i32,
) -> Result<(), sqlx::Error> {
    // Serialize pool changes for a job; safe across multiple API/worker processes.
    sqlx::query("SELECT id FROM jobs WHERE id=$1 FOR UPDATE")
        .bind(job)
        .fetch_one(&mut **tx)
        .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM question_sets WHERE job_id=$1 AND duration_minutes=$2 AND assigned_session_id IS NULL")
        .bind(job).bind(duration).fetch_one(&mut **tx).await?;
    for _ in count..POOL_SIZE {
        sqlx::query("INSERT INTO question_sets(id,interviewer_id,job_id,duration_minutes,seed) VALUES($1,$2,$3,$4,$5)")
            .bind(Uuid::new_v4()).bind(owner).bind(job).bind(duration).bind(Uuid::new_v4()).execute(&mut **tx).await?;
    }
    Ok(())
}

pub(crate) async fn assign(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    session: Uuid,
) -> Result<bool, sqlx::Error> {
    let row = sqlx::query("SELECT job_id,duration_minutes FROM interview_sessions s WHERE id=$1 AND state IN ('invited','ready') AND expires_at>clock_timestamp() AND NOT EXISTS(SELECT 1 FROM interview_questions q WHERE q.session_id=s.id) FOR UPDATE")
        .bind(session).fetch_optional(&mut **tx).await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let job: Uuid = row.get("job_id");
    let duration: i32 = row.get("duration_minutes");
    // Same lock order as replenish; no candidate can receive an already reserved set.
    sqlx::query("SELECT id FROM jobs WHERE id=$1 FOR UPDATE")
        .bind(job)
        .fetch_one(&mut **tx)
        .await?;
    let set = sqlx::query("SELECT id,seed FROM question_sets WHERE job_id=$1 AND duration_minutes=$2 AND status='completed' AND assigned_session_id IS NULL ORDER BY created_at,id FOR UPDATE SKIP LOCKED LIMIT 1")
        .bind(job).bind(duration).fetch_optional(&mut **tx).await?;
    let Some(set) = set else {
        return Ok(false);
    };
    let id: Uuid = set.get("id");
    sqlx::query("UPDATE question_sets SET assigned_session_id=$2 WHERE id=$1")
        .bind(id)
        .bind(session)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "UPDATE interview_questions SET session_id=$2 WHERE set_id=$1 AND session_id IS NULL",
    )
    .bind(id)
    .bind(session)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE interview_sessions SET generation_id=$2,generation_seed=$3,generation_status='completed',generation_error=NULL,generation_claim_token=NULL,generation_lease_until=NULL WHERE id=$1")
        .bind(session).bind(id).bind(set.get::<Uuid,_>("seed")).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'questions.assigned',$4)")
        .bind(Uuid::new_v4()).bind(owner).bind(session).bind(json!({"set_id":id,"source":"job_pool"})).execute(&mut **tx).await?;
    replenish(tx, owner, job, duration).await?;
    Ok(true)
}
