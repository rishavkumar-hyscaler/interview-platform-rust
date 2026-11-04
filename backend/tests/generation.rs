use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::State,
    http::{Request, StatusCode},
    routing::post,
};
use platform_api::{
    generation::{process_next, provider::AiProvider},
    infrastructure::interviewer_transaction,
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tower::ServiceExt;
use uuid::Uuid;

const JD: &str = "Build reliable Rust services with PostgreSQL and distributed systems.";
#[derive(Clone)]
struct Mock {
    variant: Arc<AtomicUsize>,
    malformed: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}
async fn responses(State(mock): State<Mock>, Json(input): Json<Value>) -> Json<Value> {
    mock.calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(input["model"], "gpt-5-mini");
    assert_eq!(input["store"], false);
    assert_eq!(input["text"]["format"]["strict"], true);
    let data = if input["text"]["format"]["name"] == "interview_blueprint" {
        let slots:Vec<Value>=(0..6).map(|i|json!({"competency_id":"rust","kind":if i%2==0 {"text"}else{"scenario"},"difficulty":if i<2{"easy"}else if i<4{"medium"}else{"hard"}})).collect();
        json!({"competencies":[{"id":"rust","name":"Reliable backend services","jd_evidence":if mock.malformed.load(Ordering::SeqCst)==1 {"Invented evidence which is absent"}else{JD},"weight":100}],"slots":slots})
    } else {
        let variant = mock.variant.load(Ordering::SeqCst);
        let questions:Vec<Value>=(0..6).map(|i|json!({"competency_id":"rust","kind":if i%2==0{"text"}else{"scenario"},"difficulty":if i<2{"easy"}else if i<4{"medium"}else{"hard"},"prompt":format!("Variant {variant} task {i}: explain an appropriate reliability decision for a Rust service with PostgreSQL under these constraints."),"expected_answer":"Describe the relevant tradeoffs, failure handling, and job-relevant correctness guarantees.","criteria":[{"id":"correctness","description":"Explains the relevant correctness guarantees clearly.","max_points":5},{"id":"tradeoffs","description":"Identifies concrete tradeoffs and failure handling.","max_points":if mock.malformed.load(Ordering::SeqCst)==2{4}else{5}}]})).collect();
        json!({"questions":questions})
    };
    Json(
        json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":data.to_string()}]}]}),
    )
}
async fn embeddings(Json(input): Json<Value>) -> Json<Value> {
    assert_eq!(input["model"], "text-embedding-3-small");
    assert_eq!(input["dimensions"], 12);
    let inputs = input["input"].as_array().unwrap();
    let mut data = Vec::new();
    for (index, prompt) in inputs.iter().enumerate() {
        let prompt = prompt.as_str().unwrap();
        let variant = if prompt.starts_with("Variant 1 ") {
            0
        } else {
            1
        };
        let mut embedding = vec![0.0; 12];
        embedding[variant * 6 + index] = 1.0;
        data.push(json!({"index":index,"embedding":embedding}));
    }
    // Deliberately reverse response order: the client must respect index.
    data.reverse();
    Json(json!({"data":data}))
}
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
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn account(app: &Router) -> Value {
    let (status,value)=call(app,"POST","/api/auth/register",json!({"email":format!("{}@example.test",Uuid::new_v4()),"password":"generation-test-password"}),"").await;
    assert_eq!(status, StatusCode::CREATED);
    value
}
async fn invitation(app: &Router, base: &str, job: &str, token: &str) -> Value {
    let (status,value)=call(app,"POST",&format!("{base}/jobs/{job}/invitations"),json!({"candidate_name":"Generation candidate","candidate_email":"generation@example.test","duration_minutes":30,"request_key":Uuid::new_v4()}),token).await;
    assert_eq!(status, StatusCode::CREATED);
    value
}
async fn run_due(pool: &PgPool, session: Uuid, provider: &AiProvider, owner: Uuid) {
    let mut tx = interviewer_transaction(pool, owner).await.unwrap();
    sqlx::query("UPDATE interview_sessions SET generation_available_at=now() WHERE id=$1")
        .bind(session)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(process_next(pool, provider).await.unwrap());
}
#[tokio::test]
#[ignore = "requires migrated PostgreSQL, restricted runtime role, and local mock HTTP listener"]
async fn generation_is_unique_recoverable_validated_and_private() {
    platform_api::infrastructure::load_environment().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(
            &std::env::var("RUNTIME_DATABASE_URL")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .unwrap(),
        )
        .await
        .unwrap();
    let bypass: bool = sqlx::query_scalar(
        "SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!bypass);
    let mock = Mock {
        variant: Arc::new(AtomicUsize::new(1)),
        malformed: Arc::new(AtomicUsize::new(0)),
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = Router::new()
        .route("/v1/responses", post(responses))
        .route("/v1/embeddings", post(embeddings))
        .with_state(mock.clone());
    let task = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
    let mut provider = AiProvider::new(
        format!("http://{address}/v1"),
        "mock-llm-key".into(),
        "mock-embedding-key".into(),
        "gpt-5-mini".into(),
        "text-embedding-3-small".into(),
    )
    .unwrap();
    provider.dimensions = 12;
    let app = platform_api::router_with_ai(pool.clone(), Some(provider.clone()));
    let recruiter_account = account(&app).await;
    let outsider = account(&app).await;
    let recruiter: Uuid = recruiter_account["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let token = recruiter_account["token"].as_str().unwrap();
    let other = outsider["token"].as_str().unwrap();
    let base = "/api/v1".to_string();
    let (_, job) = call(
        &app,
        "POST",
        &format!("{base}/jobs"),
        json!({"title":"Rust reliability engineer","description":JD,"request_key":Uuid::new_v4()}),
        token,
    )
    .await;
    let job = job["id"].as_str().unwrap();
    let first = invitation(&app, &base, job, token).await;
    assert_eq!(
        first["session"]["generation_status"], "pending",
        "new invitations queue automatically"
    );
    let first_id: Uuid = first["session"]["id"].as_str().unwrap().parse().unwrap();
    let candidate = first["invitation_token"].as_str().unwrap();
    let route = format!("{base}/sessions/{first_id}/generation");
    assert_eq!(
        call(&app, "POST", &route, json!({}), other).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "POST", &route, json!({}), candidate).await.0,
        StatusCode::UNAUTHORIZED
    );
    let unconfigured = platform_api::router_with_ai(pool.clone(), None);
    assert_eq!(
        call(&unconfigured, "POST", &route, json!({}), token)
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    for _ in 0..2 {
        assert_eq!(
            call(&app, "POST", &route, json!({}), token).await.0,
            StatusCode::ACCEPTED
        );
    }
    // Expiry processing must not claim or consume generation jobs.
    let _ = platform_api::infrastructure::process_expiry_job(&pool)
        .await
        .unwrap();
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    let state: String =
        sqlx::query_scalar("SELECT generation_status FROM interview_sessions WHERE id=$1")
            .bind(first_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(state, "pending");
    sqlx::query("UPDATE interview_sessions SET generation_status='running',generation_attempts=1,generation_lease_until=now()-interval '1 second',generation_claim_token=$2 WHERE id=$1").bind(first_id).bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(process_next(&pool, &provider).await.unwrap());
    let (_, prepared) = call(&app, "GET", &route, Value::Null, token).await;
    assert_eq!(prepared["status"], "completed");
    assert_eq!(prepared["questions"].as_array().unwrap().len(), 6);
    assert_eq!(prepared["job"]["attempts"], 2);
    assert_eq!(prepared["questions"][0]["rubric"]["max_score"], 10);
    assert_eq!(prepared["blueprint"]["competencies"][0]["jd_evidence"], JD);
    let calls = mock.calls.load(Ordering::SeqCst);
    assert_eq!(
        call(&app, "POST", &route, json!({}), token).await.0,
        StatusCode::OK
    );
    assert_eq!(mock.calls.load(Ordering::SeqCst), calls);
    let (_, candidate_view) = call(
        &app,
        "GET",
        "/api/v1/candidate/session",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(candidate_view["question_count"], 6);
    assert!(!candidate_view.to_string().contains("rubric"));
    assert!(!candidate_view.to_string().contains("expected_answer"));
    assert_eq!(
        call(&app, "GET", &route, Value::Null, other).await.0,
        StatusCode::NOT_FOUND
    );
    mock.variant.store(2, Ordering::SeqCst);
    let second = invitation(&app, &base, job, token).await;
    let second_id: Uuid = second["session"]["id"].as_str().unwrap().parse().unwrap();
    let second_route = format!("{base}/sessions/{second_id}/generation");
    call(&app, "POST", &second_route, json!({}), token).await;
    assert!(process_next(&pool, &provider).await.unwrap());
    let (_, other_set) = call(&app, "GET", &second_route, Value::Null, token).await;
    assert_eq!(other_set["status"], "completed");
    for q in prepared["questions"].as_array().unwrap() {
        assert!(
            !other_set["questions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|other| other["prompt"] == q["prompt"])
        );
    }
    // Semantic duplicate content with different text must also be rejected.
    mock.variant.store(3, Ordering::SeqCst);
    let third = invitation(&app, &base, job, token).await;
    let third_id: Uuid = third["session"]["id"].as_str().unwrap().parse().unwrap();
    let third_route = format!("{base}/sessions/{third_id}/generation");
    call(&app, "POST", &third_route, json!({}), token).await;
    for _ in 0..3 {
        run_due(&pool, third_id, &provider, recruiter).await;
    }
    let (_, failed) = call(&app, "GET", &third_route, Value::Null, token).await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["questions"].as_array().unwrap().len(), 0);
    assert!(
        failed["job"]["last_error"]
            .as_str()
            .unwrap()
            .contains("duplicated")
    );
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    let bank: i64 = sqlx::query_scalar("SELECT count(*) FROM interview_questions")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(bank, 12);
    tx.commit().await.unwrap();
    let invisible: i64 = sqlx::query_scalar("SELECT count(*) FROM interview_questions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(invisible, 0);
    // Bad rubric output is never partially published.
    mock.malformed.store(2, Ordering::SeqCst);
    assert_eq!(
        call(&app, "POST", &third_route, json!({}), token).await.0,
        StatusCode::ACCEPTED
    );
    run_due(&pool, third_id, &provider, recruiter).await;
    let (_, bad_rubric) = call(&app, "GET", &third_route, Value::Null, token).await;
    assert_eq!(bad_rubric["status"], "pending");
    assert!(
        bad_rubric["job"]["last_error"]
            .as_str()
            .unwrap()
            .contains("total 10")
    );
    assert_eq!(bad_rubric["questions"].as_array().unwrap().len(), 0);
    // Existing invitations accepted before this feature queue on first visit.
    let legacy = invitation(&unconfigured, &base, job, token).await;
    let legacy_id: Uuid = legacy["session"]["id"].as_str().unwrap().parse().unwrap();
    let legacy_token = legacy["invitation_token"].as_str().unwrap();
    call(
        &unconfigured,
        "POST",
        "/api/v1/candidate/session/ready",
        Value::Null,
        legacy_token,
    )
    .await;
    for _ in 0..2 {
        let (_, view) = call(
            &app,
            "GET",
            "/api/v1/candidate/session",
            Value::Null,
            legacy_token,
        )
        .await;
        assert_eq!(view["generation_status"], "pending");
        assert_eq!(view["preparation_available"], true);
    }
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='questions.queued'",
    )
    .bind(legacy_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(queued, 1, "polling must not duplicate preparation work");
    sqlx::query("UPDATE interview_sessions SET generation_status='failed' WHERE id=$1")
        .bind(legacy_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (_, failed_view) = call(
        &app,
        "GET",
        "/api/v1/candidate/session",
        Value::Null,
        legacy_token,
    )
    .await;
    assert_eq!(
        failed_view["generation_status"], "failed",
        "candidate polling must not reset failed attempts"
    );
    // No real queued work is left behind by this mock-provider test.
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    sqlx::query("UPDATE interview_sessions SET generation_status='failed' WHERE id=$1")
        .bind(third_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    task.abort();
    let _ = task.await;
    pool.close().await;
}
