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
    review_failures: Arc<AtomicUsize>,
    prompt_calls: Arc<AtomicUsize>,
    embedding_calls: Arc<AtomicUsize>,
    reference_calls: Arc<Vec<AtomicUsize>>,
    active_references: Arc<AtomicUsize>,
    peak_references: Arc<AtomicUsize>,
}
impl Mock {
    fn new() -> Self {
        Self {
            variant: Arc::new(AtomicUsize::new(1)),
            malformed: Arc::new(AtomicUsize::new(0)),
            calls: Arc::new(AtomicUsize::new(0)),
            review_failures: Arc::new(AtomicUsize::new(0)),
            prompt_calls: Arc::new(AtomicUsize::new(0)),
            embedding_calls: Arc::new(AtomicUsize::new(0)),
            reference_calls: Arc::new((0..6).map(|_| AtomicUsize::new(0)).collect()),
            active_references: Arc::new(AtomicUsize::new(0)),
            peak_references: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn references(&self) -> usize {
        self.reference_calls
            .iter()
            .map(|count| count.load(Ordering::SeqCst))
            .sum()
    }
}
fn task_index(prompt: &str) -> usize {
    prompt
        .split("task ")
        .nth(1)
        .unwrap()
        .split(':')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}
async fn responses(
    State(mock): State<Mock>,
    Json(input): Json<Value>,
) -> (StatusCode, Json<Value>) {
    mock.calls.fetch_add(1, Ordering::SeqCst);
    let malformed = mock.malformed.load(Ordering::SeqCst);
    if malformed == 7 {
        return (
            StatusCode::UNAUTHORIZED,
            Json(
                json!({"error":{"code":"invalid_api_key","message":"private provider diagnostic"}}),
            ),
        );
    }
    assert_eq!(input["model"], "gpt-5-mini");
    assert_eq!(input["store"], false);
    assert_eq!(input["text"]["format"]["strict"], true);
    let name = input["text"]["format"]["name"].as_str().unwrap();
    let request: Value =
        serde_json::from_str(input["input"][0]["content"].as_str().unwrap()).unwrap();
    let count = if name == "interview_reference_review" {
        1
    } else {
        6
    };
    if !matches!(
        name,
        "interview_reference_answer" | "interview_reference_review"
    ) {
        let array = if name == "interview_blueprint" {
            "slots"
        } else {
            "questions"
        };
        assert_eq!(
            input["text"]["format"]["schema"]["properties"][array]["minItems"],
            count
        );
        assert_eq!(
            input["text"]["format"]["schema"]["properties"][array]["maxItems"],
            count
        );
    }
    assert_eq!(
        input["reasoning"]["effort"],
        if name == "interview_reference_review" {
            "medium"
        } else {
            "low"
        }
    );
    let data = match name {
        "interview_blueprint" => {
            let id = if malformed == 8 {
                "Rust-Services"
            } else {
                "rust"
            };
            let slots:Vec<Value>=(0..6).map(|i|json!({"competency_id":id,"kind":if i%2==0 {"text"}else{"scenario"},"difficulty":if i<2{"easy"}else if i<4{"medium"}else{"hard"}})).collect();
            let bad = malformed == 1 || (malformed == 9 && request["blueprint_attempt"] == 1);
            if malformed == 9 && request["blueprint_attempt"] == 2 {
                assert!(request["previous_blueprint"].is_object());
                assert!(
                    request["repair_feedback"]
                        .as_str()
                        .unwrap()
                        .contains("quote the job description")
                );
            }
            json!({"competencies":[{"id":id,"name":"Reliable backend services","jd_evidence":if bad {"Invented evidence which is absent"}else{JD},"weight":100}],"slots":slots})
        }
        "interview_question_prompts" => {
            mock.prompt_calls.fetch_add(1, Ordering::SeqCst);
            let schema =
                &input["text"]["format"]["schema"]["$defs"]["QuestionPrompt"]["properties"];
            assert!(schema.get("expected_answer").is_none());
            assert!(schema.get("criteria").is_none());
            let variant = mock.variant.load(Ordering::SeqCst);
            let mut questions:Vec<Value>=request["blueprint"]["slots"].as_array().unwrap().iter().enumerate().map(|(i,slot)|json!({"competency_id":slot["competency_id"],"kind":slot["kind"],"difficulty":slot["difficulty"],"prompt":format!("Variant {variant} task {i}: explain an appropriate reliability decision for a Rust service with PostgreSQL under these constraints."),"estimated_minutes":if malformed==5 {30}else{4}})).collect();
            if malformed == 12 {
                questions[1]["prompt"] = questions[0]["prompt"].clone();
            }
            json!({"questions":questions})
        }
        "interview_prompt_review" => {
            let questions:Vec<Value>=(1..=6).map(|position|json!({"position":position,"jd_grounded":malformed!=13,"feasible":true,"unambiguous":true,"no_answer_leak":true,"concerns":if malformed==13 {vec!["Question needs concrete job-relevant inputs"]} else {vec![]}})).collect();
            json!({"questions":questions})
        }
        "interview_reference_answer" => {
            let approved = &request["approved_question"];
            let index = task_index(approved["prompt"].as_str().unwrap());
            mock.reference_calls[index].fetch_add(1, Ordering::SeqCst);
            let active = mock.active_references.fetch_add(1, Ordering::SeqCst) + 1;
            mock.peak_references.fetch_max(active, Ordering::SeqCst);
            // Hold responses briefly so the test detects unbounded or sequential requests.
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            mock.active_references.fetch_sub(1, Ordering::SeqCst);
            let schema = &input["text"]["format"]["schema"]["properties"];
            assert!(
                schema.get("prompt").is_none(),
                "answers cannot alter approved questions"
            );
            let attempt = request["reference_attempt"].as_u64().unwrap();
            if malformed == 11 && attempt > 1 {
                assert!(
                    request["repair_feedback"]
                        .as_str()
                        .unwrap()
                        .contains("Explain rollback after a partial write")
                );
            }
            if malformed == 6 && index == 0 && attempt > 1 {
                assert!(request["previous_reference"].is_object());
                assert!(
                    request["repair_feedback"]
                        .as_str()
                        .unwrap()
                        .contains("worked reference section")
                );
            }
            let bad = malformed == 3 || (malformed == 6 && index == 0 && attempt == 1);
            let mut reference = json!({"expected_answer":"Use an atomic transaction to commit the durable state together. Retry transient errors with bounded exponential backoff and jitter. Compare the consistency and availability tradeoffs, and make the operation idempotent to prevent duplicate side effects.","assumptions":["Assume a transactional PostgreSQL backend."],"acceptable_alternatives":["A transactional outbox is acceptable when delivery must occur outside the database transaction."],"common_mistakes":["Retrying a non-idempotent operation can apply a side effect twice."],"coding":null,"criteria":[{"id":"correctness","description":"Explains the relevant correctness guarantees clearly.","max_points":5,"partial_credit":"Identifies atomicity but omits failure handling details.","full_credit":"Explains how an atomic transaction preserves the durable state.","reference_answer":"Use an atomic transaction to commit the durable state together."},{"id":"tradeoffs","description":"Identifies concrete tradeoffs and failure handling.","max_points":if malformed==2 {4}else{5},"partial_credit":"Names a tradeoff without connecting it to this scenario.","full_credit":"Connects consistency and availability tradeoffs to idempotent retries.","reference_answer":if bad {"Missing"}else{"Compare the consistency and availability tradeoffs"}}]});
            assert_eq!(
                input["text"]["format"]["schema"]["$defs"]["Criterion"]["properties"]["id"]["pattern"],
                "^[a-z0-9_]{1,40}$"
            );
            if index == 0 && (malformed == 15 || (malformed == 14 && attempt == 1)) {
                reference["criteria"][0]["id"] = json!("Correctness-Score");
            }
            if malformed == 14 && index == 0 && attempt == 2 {
                assert!(
                    request["repair_feedback"]
                        .as_str()
                        .unwrap()
                        .contains("criterion 1 ID must")
                );
                assert!(request["previous_reference"].is_object());
            }
            if malformed == 10 {
                reference["prompt"] = json!("An unauthorized replacement prompt");
            }
            reference
        }
        "interview_reference_review" => {
            let reject = mock
                .review_failures
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok();
            assert_eq!(
                request["proposed_set"]["questions"]
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            {
                let properties = &input["text"]["format"]["schema"]["properties"];
                assert!(properties.get("position").is_none());
                assert!(
                    properties.get("questions").is_none(),
                    "model must not author question identity in a single-reference verdict"
                );
                json!({"jd_grounded":true,"reference_correct":malformed!=4 && !reject,"rubric_aligned":true,"feasible":true,"unambiguous":true,"no_answer_leak":true,"concerns":if reject {vec!["Explain rollback after a partial write"]}else{vec![]}})
            }
        }
        _ => panic!("Unexpected generation stage: {name}"),
    };
    (
        StatusCode::OK,
        Json(
            json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":data.to_string()}]}]}),
        ),
    )
}
async fn embeddings(State(mock): State<Mock>, Json(input): Json<Value>) -> Json<Value> {
    mock.embedding_calls.fetch_add(1, Ordering::SeqCst);
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
        if prompt.starts_with("Variant 4 ") {
            embedding[index] = -1.0;
        } else {
            embedding[variant * 6 + index] = 1.0;
        }
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
    let mock = Mock::new();
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
        json!({"job_code":format!("TEST-{}",Uuid::new_v4()),"title":"Rust reliability engineer","description":JD,"request_key":Uuid::new_v4()}),
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
    for question in prepared["questions"].as_array().unwrap() {
        assert_eq!(
            question["rubric"]["quality_review"]["position"], question["position"],
            "review identity must be assigned by the server, including questions 2–6"
        );
    }
    assert_eq!(
        mock.peak_references.load(Ordering::SeqCst),
        3,
        "reference requests must run concurrently with a bound of three"
    );
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
    // Exact bank collisions are rejected even before paying for embeddings.
    let duplicate = invitation(&app, &base, job, token).await;
    let duplicate_id: Uuid = duplicate["session"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let references_before = mock.references();
    let embeddings_before = mock.embedding_calls.load(Ordering::SeqCst);
    run_due(&pool, duplicate_id, &provider, recruiter).await;
    assert_eq!(mock.references(), references_before);
    assert_eq!(
        mock.embedding_calls.load(Ordering::SeqCst),
        embeddings_before
    );
    // Semantic duplicate content with different text must also be rejected.
    mock.variant.store(3, Ordering::SeqCst);
    let third = invitation(&app, &base, job, token).await;
    let third_id: Uuid = third["session"]["id"].as_str().unwrap().parse().unwrap();
    let third_route = format!("{base}/sessions/{third_id}/generation");
    call(&app, "POST", &third_route, json!({}), token).await;
    let references_before = mock.references();
    run_due(&pool, third_id, &provider, recruiter).await;
    assert_eq!(
        mock.references(),
        references_before,
        "semantic duplicates must be rejected before generating any answers"
    );
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
    mock.variant.store(4, Ordering::SeqCst);
    mock.malformed.store(2, Ordering::SeqCst);
    assert_eq!(
        call(&app, "POST", &third_route, json!({}), token).await.0,
        StatusCode::ACCEPTED
    );
    run_due(&pool, third_id, &provider, recruiter).await;
    let (_, bad_rubric) = call(&app, "GET", &third_route, Value::Null, token).await;
    assert_eq!(bad_rubric["status"], "failed");
    assert!(
        bad_rubric["job"]["last_error"]
            .as_str()
            .unwrap()
            .contains("total 10")
    );
    assert_eq!(bad_rubric["questions"].as_array().unwrap().len(), 0);
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    sqlx::query("UPDATE interview_sessions SET generation_status='failed' WHERE id=$1")
        .bind(third_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    // Reference alignment, critique rejection, timing and bounded repair are independently exercised.
    for malformed in [1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15] {
        mock.malformed.store(malformed, Ordering::SeqCst);
        mock.review_failures
            .store(if malformed == 11 { 1 } else { 0 }, Ordering::SeqCst);
        let (_, fresh_job) = call(
            &app,
            "POST",
            &format!("{base}/jobs"),
            json!({"job_code":format!("TEST-{}",Uuid::new_v4()),"title":"Reference quality case","description":JD,"request_key":Uuid::new_v4()}),
            token,
        )
        .await;
        let fresh = invitation(&app, &base, fresh_job["id"].as_str().unwrap(), token).await;
        let id: Uuid = fresh["session"]["id"].as_str().unwrap().parse().unwrap();
        let references_before = mock.references();
        let calls_before: Vec<usize> = mock
            .reference_calls
            .iter()
            .map(|n| n.load(Ordering::SeqCst))
            .collect();
        let prompts_before = mock.prompt_calls.load(Ordering::SeqCst);
        run_due(&pool, id, &provider, recruiter).await;
        if matches!(malformed, 1 | 5 | 7 | 12 | 13) {
            assert_eq!(
                mock.references(),
                references_before,
                "invalid or rejected prompts must never incur reference-generation calls"
            );
        }
        if matches!(malformed, 6 | 11 | 14) {
            assert_eq!(
                mock.prompt_calls.load(Ordering::SeqCst) - prompts_before,
                1,
                "reference repair must reuse approved prompts"
            );
            let counts: Vec<usize> = mock
                .reference_calls
                .iter()
                .zip(calls_before)
                .map(|(n, before)| n.load(Ordering::SeqCst) - before)
                .collect();
            assert_eq!(
                counts.iter().filter(|n| **n == 2).count(),
                1,
                "only the rejected reference is regenerated"
            );
            assert_eq!(counts.iter().filter(|n| **n == 1).count(), 5);
        }
        let (_, result) = call(
            &app,
            "GET",
            &format!("{base}/sessions/{id}/generation"),
            Value::Null,
            token,
        )
        .await;
        if matches!(malformed, 6 | 8 | 9 | 11 | 14) {
            assert_eq!(result["status"], "completed");
            assert_eq!(
                result["questions"][0]["rubric"]["quality_review"]["reference_correct"],
                true
            );
        } else {
            assert_eq!(result["status"], "failed");
            assert!(result["questions"].as_array().unwrap().is_empty());
            let message = result["job"]["last_error"].as_str().unwrap();
            assert!(match malformed {
                1 => message.contains("quote the job description"),
                3 => message.contains("worked reference section"),
                4 => message.contains("Quality review rejected"),
                5 => message.contains("duration"),
                7 => message.contains("invalid_api_key") && !message.contains("private provider"),
                10 => message.contains("required schema"),
                12 => message.contains("duplicate"),
                13 => message.contains("prompt review"),
                15 => message.contains("criterion 1 ID must"),
                _ => false,
            });
        }
        let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
        sqlx::query("UPDATE interview_sessions SET generation_status='failed' WHERE id=$1 AND generation_status<>'completed'").bind(id).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
    mock.malformed.store(0, Ordering::SeqCst);
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

#[tokio::test]
#[ignore = "requires migrated PostgreSQL, restricted runtime role, and local mock HTTP listener"]
async fn job_pool_prepares_before_invitations_and_assigns_once() {
    platform_api::infrastructure::load_environment().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&std::env::var("RUNTIME_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // This suite uses a disposable database. Isolate this provider's work from other fixtures.
    let owners: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer'")
        .fetch_all(&pool)
        .await
        .unwrap();
    for owner in owners {
        let mut tx = interviewer_transaction(&pool, owner).await.unwrap();
        sqlx::query(
            "UPDATE question_sets SET available_at=now()+interval '1 day' WHERE status='pending'",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let mock = Mock::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = Router::new()
        .route("/v1/responses", post(responses))
        .route("/v1/embeddings", post(embeddings))
        .with_state(mock.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    let mut provider = AiProvider::new(
        format!("http://{address}/v1"),
        "mock-key".into(),
        "mock-key".into(),
        "gpt-5-mini".into(),
        "text-embedding-3-small".into(),
    )
    .unwrap();
    provider.dimensions = 12;
    let app = platform_api::router_with_ai(pool.clone(), Some(provider.clone()));
    let owner = account(&app).await;
    let other = account(&app).await;
    let token = owner["token"].as_str().unwrap();
    let recruiter: Uuid = owner["user_id"].as_str().unwrap().parse().unwrap();
    let (_, older_job) = call(&app,"POST","/api/v1/jobs",json!({"job_code":format!("OLDER-{}",Uuid::new_v4()),"title":"Older queued role","description":JD,"request_key":Uuid::new_v4()}),token).await;
    let input = json!({"job_code":format!("POOL-{}",Uuid::new_v4()),"title":"Prepared Rust role","description":JD,"preparation_duration_minutes":30,"request_key":Uuid::new_v4()});
    let (status, job) = call(&app, "POST", "/api/v1/jobs", input.clone(), token).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        call(&app, "POST", "/api/v1/jobs", input, token).await.0,
        StatusCode::OK
    );
    let id = job["id"].as_str().unwrap();
    let route = format!("/api/v1/jobs/{id}/question-sets");
    let (_, initial) = call(&app, "GET", &route, Value::Null, token).await;
    assert_eq!(
        initial["sets"].as_array().unwrap().len(),
        3,
        "idempotent job creation queues exactly three sets"
    );
    assert!(initial["questions"].as_array().unwrap().is_empty());
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    let sessions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM interview_sessions WHERE job_id=$1")
            .bind(id.parse::<Uuid>().unwrap())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        sessions, 0,
        "preparation must not fabricate candidate sessions"
    );
    let first: Uuid = initial["sets"][0]["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE question_sets SET status='running',attempts=1,claim_token=$2,lease_until=now()-interval '1 second' WHERE id=$1").bind(first).bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for variant in [1, 2] {
        mock.variant.store(variant, Ordering::SeqCst);
        assert!(process_next(&pool, &provider).await.unwrap());
    }
    let (_, older_status) = call(
        &app,
        "GET",
        &format!(
            "/api/v1/jobs/{}/question-sets",
            older_job["id"].as_str().unwrap()
        ),
        Value::Null,
        token,
    )
    .await;
    assert!(
        older_status["sets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|set| set["attempts"] == 0),
        "new job's fresh work should precede the older backlog"
    );
    // Once fresh stock is served, expired leases remain recoverable on retry.
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    sqlx::query("UPDATE question_sets SET available_at=now()+interval '1 day' WHERE job_id=$1")
        .bind(older_job["id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    mock.variant.store(4, Ordering::SeqCst);
    assert!(process_next(&pool, &provider).await.unwrap());
    let (_, ready) = call(&app, "GET", &route, Value::Null, token).await;
    assert_eq!(ready["questions"].as_array().unwrap().len(), 18);
    assert!(
        ready["sets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|set| set["status"] == "completed")
    );
    assert!(
        ready["questions"][0]["rubric"]["expected_answer"]
            .as_str()
            .unwrap()
            .len()
            > 100
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &route,
            Value::Null,
            other["token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert!(
        ready["sets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|set| set["id"] == first.to_string() && set["attempts"] == 2)
    );
    let invisible: i64 = sqlx::query_scalar("SELECT count(*) FROM question_sets")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        invisible, 0,
        "pool rows require an authenticated ownership scope"
    );
    let calls = mock.calls.load(Ordering::SeqCst);
    let (a, b) = tokio::join!(
        invitation(&app, "/api/v1", id, token),
        invitation(&app, "/api/v1", id, token)
    );
    assert_eq!(a["session"]["generation_status"], "completed");
    assert_eq!(b["session"]["generation_status"], "completed");
    assert_ne!(a["session"]["generation_id"], b["session"]["generation_id"]);
    assert_eq!(
        mock.calls.load(Ordering::SeqCst),
        calls,
        "assigning prepared sets must not call AI"
    );
    let candidate = a["invitation_token"].as_str().unwrap();
    let (_, view) = call(
        &app,
        "GET",
        "/api/v1/candidate/session",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(view["question_count"], 6);
    assert!(!view.to_string().contains("expected_answer"));
    assert_eq!(
        call(&app, "GET", &route, Value::Null, candidate).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, remaining) = call(&app, "GET", &route, Value::Null, token).await;
    assert_eq!(
        remaining["sets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["assigned"] == false)
            .count(),
        3
    );
    assert_eq!(
        remaining["questions"].as_array().unwrap().len(),
        6,
        "only unused questions appear in pool preview"
    );
    let unconfigured = platform_api::router_with_ai(pool.clone(), None);
    let third = invitation(&unconfigured, "/api/v1", id, token).await;
    assert_eq!(
        third["session"]["generation_status"], "completed",
        "existing sets work even with provider offline"
    );
    let fourth = invitation(&app, "/api/v1", id, token).await;
    assert_eq!(
        fourth["session"]["generation_status"], "pending",
        "empty pool queues unique session generation"
    );
    // Failed jobs remain bounded until an authorized explicit retry.
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    sqlx::query("UPDATE interview_sessions SET generation_status='failed' WHERE id=$1")
        .bind(
            fourth["session"]["id"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()
                .unwrap(),
        )
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE question_sets SET status='failed',attempts=3,last_error='Synthetic quality failure' WHERE job_id=$1 AND assigned_session_id IS NULL").bind(id.parse::<Uuid>().unwrap()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    sqlx::query("UPDATE question_sets SET status='pending',attempts=0,available_at=now() WHERE id=(SELECT id FROM question_sets WHERE job_id=$1 AND assigned_session_id IS NULL ORDER BY id LIMIT 1)")
        .bind(id.parse::<Uuid>().unwrap()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    mock.malformed.store(7, Ordering::SeqCst);
    assert!(process_next(&pool, &provider).await.unwrap());
    let (_, failure) = call(&app, "GET", &route, Value::Null, token).await;
    assert!(
        failure["sets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|set| set["status"] == "failed" && set["attempts"] == 1)
    );
    assert_eq!(
        call(&unconfigured, "POST", &route, json!({}), token)
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        call(&app, "POST", &route, json!({}), token).await.0,
        StatusCode::ACCEPTED
    );
    let (_, retried) = call(&app, "GET", &route, Value::Null, token).await;
    assert!(
        retried["sets"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["assigned"] == false)
            .all(|s| s["status"] == "pending" && s["attempts"] == 0)
    );
    task.abort();
    let _ = task.await;
    pool.close().await;
}
