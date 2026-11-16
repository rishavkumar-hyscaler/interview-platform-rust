//! Database fixtures for HTTP tests that exercise features after generation.
//! Actual worker validation/publication is covered by generation.rs.
use platform_api::{infrastructure::interviewer_transaction, invitations::InvitationCipher};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn seed_set(pool: &PgPool, owner: Uuid, job: Uuid, duration: i32) {
    let mut tx = interviewer_transaction(pool, owner)
        .await
        .unwrap();
    sqlx::query("UPDATE jobs SET blueprint='{}'::jsonb WHERE id=$1")
        .bind(job)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO question_sets(id,interviewer_id,job_id,duration_minutes,seed) VALUES($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(owner).bind(job).bind(duration).bind(Uuid::new_v4())
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

pub async fn publish_fixture(pool: &PgPool, owner: Uuid, invite: &mut Value) {
    let session: Uuid = invite["session"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let sealed = InvitationCipher::from_environment()
        .unwrap()
        .seal(&token, owner, session)
        .unwrap();
    let mut tx = interviewer_transaction(pool, owner)
        .await
        .unwrap();
    // This fixture already contains explicit question IDs referenced by the test.
    // Attach it to the candidate and emulate the worker's publication transaction.
    sqlx::query("UPDATE interview_sessions s SET question_set_id=(SELECT p.id FROM question_sets p JOIN interview_questions q ON q.set_id=p.id WHERE p.job_id=s.job_id AND p.status='completed' ORDER BY p.created_at,p.id LIMIT 1), expires_at=clock_timestamp()+interval '7 days' WHERE s.id=$1")
        .bind(session).execute(&mut *tx).await.unwrap();
    let expires: Value =
        sqlx::query_scalar("SELECT to_jsonb(expires_at) FROM interview_sessions WHERE id=$1")
            .bind(session)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query("INSERT INTO auth_sessions(token_hash,user_id,interviewer_id,interview_session_id,expires_at,sealed_token) SELECT $1,interviewee_id,interviewer_id,id,expires_at,$3 FROM interview_sessions WHERE id=$2")
        .bind(hex::encode(Sha256::digest(token.as_bytes()))).bind(session).bind(sealed)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    invite["invitation_token"] = json!(token);
    invite["session"]["state"] = json!("invited");
    invite["session"]["expires_at"] = json!(expires);
}
