use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use platform_api::infrastructure::interviewer_transaction;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;
async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Value,
    token: &str,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 262144)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn account(app: &Router) -> Value {
    let(s,v)=call(app,"POST","/api/auth/register",json!({"email":format!("{}@example.test",Uuid::new_v4()),"password":"interview-room-test-password"}),"").await;
    assert_eq!(s, StatusCode::CREATED);
    v
}
async fn setup(pool: &PgPool, app: &Router, owner: &Value) -> (Value, Vec<Uuid>) {
    let token = owner["token"].as_str().unwrap();
    let user: Uuid = owner["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let(s,job)=call(app,"POST","/api/v1/jobs",json!({"job_code":format!("TEST-{}",Uuid::new_v4()),"title":"Interview room test","description":"Build reliable applications in Rust and PostgreSQL.","request_key":Uuid::new_v4()}),token).await;
    assert_eq!(s, StatusCode::CREATED);
    let job: Uuid = job["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let ids = vec![Uuid::new_v4(), Uuid::new_v4()];
    let mut tx = interviewer_transaction(pool, user)
        .await
        .unwrap();
    let id: Uuid = sqlx::query_scalar(
        "SELECT id FROM question_sets WHERE job_id=$1 ORDER BY created_at,id LIMIT 1",
    )
    .bind(job)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    for (i, q) in ids.iter().enumerate() {
        sqlx::query("INSERT INTO interview_questions(id,interviewer_id,set_id,job_id,fingerprint,position,kind,difficulty,prompt,rubric) VALUES($1,$2,$3,$4,$5,$6,$7,'medium','Explain a reliable solution with clear tradeoffs','{\"criteria\":[{\"id\":\"correctness\",\"max_points\":10}],\"max_score\":10}')").bind(q).bind(user).bind(id).bind(job).bind(q.to_string()).bind((i+1)as i32).bind(if i==0{"text"}else{"code"}).execute(&mut *tx).await.unwrap();
    }
    sqlx::query("UPDATE question_sets SET status='completed' WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let(s,invite)=call(app,"POST",&format!("/api/v1/jobs/{job}/invitations"),json!({"candidate_name":"Room test candidate","candidate_email":"room-test@example.test","duration_minutes":30,"request_key":Uuid::new_v4()}),token).await;
    assert_eq!(s, StatusCode::CREATED);
    (invite, ids)
}

#[tokio::test]
#[ignore = "requires migrated PostgreSQL and restricted runtime credentials"]
async fn blank_answers_are_zero_scoring_is_idempotent_and_private() {
    platform_api::infrastructure::load_environment().unwrap();
    let pool = PgPoolOptions::new()
        .connect(&std::env::var("RUNTIME_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let app = platform_api::router_with_ai(pool.clone(), None);
    let owner = account(&app).await;
    let (invite, _) = setup(&pool, &app, &owner).await;
    let session: Uuid = invite["session"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let owner_id: Uuid = owner["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let candidate = invite["invitation_token"]
        .as_str()
        .unwrap();
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    sqlx::query("UPDATE interview_sessions SET state='in_progress',started_at=now(),deadline_at=now()+interval '30 minutes' WHERE id=$1").bind(session).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let event = Uuid::new_v4();
    let batch = json!([{"id":event,"kind":"paste","client_time":1000,"count":200}]);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/candidate/events",
            batch.clone(),
            candidate
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(&app, "POST", "/api/v1/candidate/events", batch, candidate)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/candidate/events",
            json!([{"id":event,"kind":"paste","client_time":1000,"count":201}]),
            candidate
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let integrity_path = format!("/api/v1/sessions/{session}/integrity");
    let (_, risk) = call(
        &app,
        "GET",
        &integrity_path,
        Value::Null,
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert_eq!(risk["events"].as_array().unwrap().len(), 1);
    assert_eq!(risk["events"][0]["chain_status"], "valid_server_receipt");
    assert_eq!(risk["risk_score"], 2);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/sessions/{session}/annotations"),
            json!({"event_id":event,"decision":"dismissed","note":"Permitted paste of own notes"}),
            owner["token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, risk) = call(
        &app,
        "GET",
        &integrity_path,
        Value::Null,
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert_eq!(risk["risk_score"], 0);
    let outsider = account(&app).await;
    assert_eq!(
        call(
            &app,
            "GET",
            &integrity_path,
            Value::Null,
            outsider["token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (status, export) = call(
        &app,
        "GET",
        &format!("/api/v1/sessions/{session}/report"),
        Value::Null,
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        export["questions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    sqlx::query("UPDATE interview_sessions SET state='submitted',submitted_at=now() WHERE id=$1")
        .bind(session)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let provider = platform_api::generation::provider::AiProvider::new(
        "http://127.0.0.1:1".into(),
        "test".into(),
        "test".into(),
        "test".into(),
        "test".into(),
    )
    .unwrap();
    for _ in 0..100 {
        if !platform_api::scoring::process_next(&pool, &provider)
            .await
            .unwrap()
        {
            break;
        }
    }
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='assessment.completed'",
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count, 1);
    tx.commit().await.unwrap();
    let path = format!("/api/v1/sessions/{session}/assessment");
    let (status, report) = call(
        &app,
        "GET",
        &path,
        Value::Null,
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["state"], "completed");
    let (status, metrics) = call(
        &app,
        "GET",
        "/api/v1/metrics",
        Value::Null,
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(metrics["sessions"].as_i64().unwrap() > 0);
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    let job: Uuid = sqlx::query_scalar("SELECT job_id FROM interview_sessions WHERE id=$1")
        .bind(session)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (status, analytics) = call(
        &app,
        "GET",
        &format!("/api/v1/jobs/{job}/analytics"),
        Value::Null,
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !analytics["groups"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    assert_eq!(report["assessment"]["questions"][0]["score"], 0);
    let (status, _) = call(
        &app,
        "GET",
        &path,
        Value::Null,
        invite["invitation_token"]
            .as_str()
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[ignore = "requires disposable migrated PostgreSQL; provider is unreachable loopback only"]
async fn failed_scoring_retries_promptly_and_reuses_validated_checkpoints() {
    platform_api::infrastructure::load_environment().unwrap();
    let pool = PgPoolOptions::new()
        .connect(&std::env::var("RUNTIME_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let app = platform_api::router_with_ai(pool.clone(), None);
    let owner = account(&app).await;
    let (invite, ids) = setup(&pool, &app, &owner).await;
    let session: Uuid = invite["session"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let owner_id: Uuid = owner["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let token = owner["token"].as_str().unwrap();
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    sqlx::query("UPDATE interview_sessions SET state='submitted',submitted_at=now() WHERE id=$1")
        .bind(session)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO interview_answers(id,interviewer_id,session_id,question_id,content) VALUES($1,$2,$3,$4,'Local synthetic answer')").bind(Uuid::new_v4()).bind(owner_id).bind(session).bind(ids[1]).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let provider = platform_api::generation::provider::AiProvider::new(
        "http://127.0.0.1:1".into(),
        "mock".into(),
        "mock".into(),
        "mock".into(),
        "mock".into(),
    )
    .unwrap();
    assert!(
        platform_api::scoring::process_next(&pool, &provider)
            .await
            .unwrap()
    );
    let path = format!("/api/v1/sessions/{session}/assessment");
    let (_, report) = call(&app, "GET", &path, Value::Null, token).await;
    assert_eq!(report["state"], "submitted");
    assert_eq!(report["attempts"], 1);
    assert_eq!(report["last_failure"]["question_position"], 2);
    assert_eq!(
        report["last_failure"]["reason"],
        "AI service connection failed or timed out"
    );
    assert!(
        !platform_api::scoring::process_next(&pool, &provider)
            .await
            .unwrap(),
        "backoff prevents a hot loop"
    );
    // Wait only the short retry delay; the original started claim remains fresh.
    tokio::time::sleep(std::time::Duration::from_secs(16)).await;
    assert!(
        platform_api::scoring::process_next(&pool, &provider)
            .await
            .unwrap(),
        "explicit failure must not hold a 15 minute lease"
    );
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    let reused:i64=sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='assessment.question.scored' AND metadata->>'reused'='true'").bind(session).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(reused, 1);
    tx.commit().await.unwrap();
    // Explicit retry is also allowed immediately after an ended attempt.
    assert_eq!(
        call(&app, "POST", &format!("{path}/retry"), json!({}), token)
            .await
            .0,
        StatusCode::OK
    );
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    sqlx::query("UPDATE interview_answers SET content='' WHERE session_id=$1")
        .bind(session)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        platform_api::scoring::process_next(&pool, &provider)
            .await
            .unwrap()
    );
    let (_, report) = call(&app, "GET", &path, Value::Null, token).await;
    assert_eq!(report["state"], "completed");
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    let queued: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM embedding_jobs WHERE session_id=$1 AND status='pending')",
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(queued, "scoring completion queues embeddings independently");
    tx.commit().await.unwrap();
}
