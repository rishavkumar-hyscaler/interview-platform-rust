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
        sqlx::query("INSERT INTO interview_questions(id,interviewer_id,set_id,job_id,fingerprint,position,kind,difficulty,prompt,rubric) VALUES($1,$2,$3,$4,$5,$6,$7,'medium','Explain a reliable solution with clear tradeoffs','{\"secret_expected_answer\":\"must never leak\"}')").bind(q).bind(user).bind(id).bind(job).bind(q.to_string()).bind((i+1)as i32).bind(if i==0{"text"}else{"code"}).execute(&mut *tx).await.unwrap();
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
async fn timed_room_saves_resume_conflicts_and_deadlines_are_enforced() {
    platform_api::infrastructure::load_environment().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect(
            &std::env::var("RUNTIME_DATABASE_URL")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .unwrap(),
        )
        .await
        .unwrap();
    let app = platform_api::router_with_ai(pool.clone(), None);
    let owner = account(&app).await;
    let other = account(&app).await;
    let owner_id: Uuid = owner["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let token = owner["token"].as_str().unwrap();
    let (invite, questions) = setup(&pool, &app, &owner).await;
    let (other_invite, other_questions) = setup(&pool, &app, &other).await;
    let candidate = invite["invitation_token"]
        .as_str()
        .unwrap();
    let other_candidate = other_invite["invitation_token"]
        .as_str()
        .unwrap();
    let session: Uuid = invite["session"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/candidate/room",
            Value::Null,
            candidate
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    call(
        &app,
        "POST",
        "/api/v1/candidate/session/ready",
        Value::Null,
        candidate,
    )
    .await;
    let (s, started) = call(
        &app,
        "POST",
        "/api/v1/candidate/session/start",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, restarted) = call(
        &app,
        "POST",
        "/api/v1/candidate/session/start",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(started["deadline_at"], restarted["deadline_at"]);
    let (s, room) = call(
        &app,
        "GET",
        "/api/v1/candidate/room",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(room["question"]["position"], 1);
    assert!(
        !room
            .to_string()
            .contains("must never leak")
    );
    assert!(!room.to_string().contains("rubric"));
    let answer = format!("/api/v1/candidate/answers/{}", questions[0]);
    let input = json!({"content":"Use transactions and clear retry boundaries.","revision":0});
    let (s, saved) = call(&app, "POST", &answer, input.clone(), candidate).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(saved["revision"], 1);
    assert_eq!(saved["answer_count"], 1);
    let (_, duplicate) = call(&app, "POST", &answer, input, candidate).await;
    assert_eq!(duplicate["revision"], 1);
    assert_eq!(
        call(
            &app,
            "POST",
            &answer,
            json!({"content":"A stale replacement","revision":0}),
            candidate
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/candidate/answers/{}", other_questions[0]),
            json!({"content":"Cross-session write","revision":0}),
            candidate
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &answer,
            json!({"content":"Forged answer","revision":0}),
            token
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    call(
        &app,
        "POST",
        "/api/v1/candidate/session/ready",
        Value::Null,
        other_candidate,
    )
    .await;
    call(
        &app,
        "POST",
        "/api/v1/candidate/session/start",
        Value::Null,
        other_candidate,
    )
    .await;
    assert_eq!(
        call(
            &app,
            "POST",
            &answer,
            json!({"content":"Other candidate write","revision":0}),
            other_candidate
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (_, next) = call(
        &app,
        "GET",
        "/api/v1/candidate/room?position=2",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(next["question"]["kind"], "code");
    let (_, resumed) = call(
        &app,
        "GET",
        "/api/v1/candidate/room",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(resumed["question"]["position"], 2);
    assert_eq!(resumed["session"]["deadline_at"], started["deadline_at"]);
    let (_, previous) = call(
        &app,
        "GET",
        "/api/v1/candidate/room?position=1",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(
        previous["question"]["answer"],
        "Use transactions and clear retry boundaries."
    );
    assert_eq!(previous["question"]["revision"], 1);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/candidate/room?position=3",
            Value::Null,
            candidate
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &answer,
            json!({"content":"x".repeat(48001),"revision":1}),
            candidate
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, review) = call(
        &app,
        "GET",
        &format!("/api/v1/sessions/{session}/generation"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(
        review["questions"][0]["answer"]["content"],
        "Use transactions and clear retry boundaries."
    );
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    let saves: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='answer.saved'",
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(saves, 1);
    sqlx::query("UPDATE interview_sessions SET deadline_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(session).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &answer,
            json!({"content":"Too late","revision":1}),
            candidate
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, ended) = call(
        &app,
        "GET",
        "/api/v1/candidate/session",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(ended["state"], "submitted");
    assert_eq!(ended["answer_count"], 1);
    let (_, closed) = call(
        &app,
        "GET",
        "/api/v1/candidate/room",
        Value::Null,
        candidate,
    )
    .await;
    assert!(closed["question"].is_null());
    for _ in 0..2 {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/candidate/session/submit",
                Value::Null,
                candidate
            )
            .await
            .1["state"],
            "submitted"
        );
    }
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    let events: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='session.timed_out'",
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(events, 1);
    let value: String =
        sqlx::query_scalar("SELECT content FROM interview_answers WHERE session_id=$1")
            .bind(session)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(value, "Use transactions and clear retry boundaries.");
    tx.commit().await.unwrap();
    pool.close().await;
}
