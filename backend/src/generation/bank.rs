//! Prepare a fixed number of reusable job sets and link each session directly to a set.
use serde_json::json;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

pub(crate) async fn replenish(
    tx: &mut Transaction<'_, Postgres>,
    owner: Uuid,
    job: Uuid,
    duration: i32,
) -> Result<(), sqlx::Error> {
    // Serialize pool changes for a job; safe across multiple API/worker processes.
    let target: i32 =
        sqlx::query_scalar("SELECT question_set_count FROM jobs WHERE id=$1 FOR UPDATE")
            .bind(job)
            .fetch_one(&mut **tx)
            .await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM question_sets WHERE job_id=$1")
        .bind(job)
        .fetch_one(&mut **tx)
        .await?;
    for _ in count..i64::from(target) {
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
    let row = sqlx::query("SELECT job_id,duration_minutes FROM interview_sessions s WHERE id=$1 AND state IN ('invited','ready') AND expires_at>clock_timestamp() AND s.question_set_id IS NULL AND NOT EXISTS(SELECT 1 FROM session_question_details() q WHERE q.session_id=s.id) FOR UPDATE")
        .bind(session).fetch_optional(&mut **tx).await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let job: Uuid = row.get("job_id");
    let duration: i32 = row.get("duration_minutes");
    // Serialize usage balancing so concurrent invitations choose the least-used set.
    sqlx::query("SELECT id FROM jobs WHERE id=$1 FOR UPDATE")
        .bind(job)
        .fetch_one(&mut **tx)
        .await?;
    let set = sqlx::query("SELECT p.id FROM question_sets p WHERE p.job_id=$1 AND p.duration_minutes=$2 AND p.status='completed' AND EXISTS(SELECT 1 FROM interview_questions q WHERE q.set_id=p.id) AND p.id IN (SELECT id FROM question_sets WHERE job_id=p.job_id ORDER BY created_at,id LIMIT (SELECT question_set_count FROM jobs WHERE id=p.job_id)) ORDER BY (SELECT count(*) FROM interview_sessions s WHERE s.question_set_id=p.id),p.created_at,p.id FOR UPDATE OF p LIMIT 1")
        .bind(job).bind(duration).fetch_optional(&mut **tx).await?;
    let Some(set) = set else {
        return Ok(false);
    };
    let id: Uuid = set.get("id");
    sqlx::query("UPDATE interview_sessions SET question_set_id=$2 WHERE id=$1")
        .bind(session)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO audit_events(id,interviewer_id,entity_id,action,metadata) VALUES($1,$2,$3,'questions.assigned',$4)")
        .bind(Uuid::new_v4()).bind(owner).bind(session).bind(json!({"set_id":id,"source":"job_pool","reused":true})).execute(&mut **tx).await?;
    Ok(true)
}
