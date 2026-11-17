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
    prompt_reviews: Arc<Vec<AtomicUsize>>,
    prompt_sizes: Arc<std::sync::Mutex<Vec<usize>>>,
    prompt_failures: Arc<AtomicUsize>,
    embedding_calls: Arc<AtomicUsize>,
    reference_calls: Arc<Vec<AtomicUsize>>,
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
            prompt_reviews: Arc::new(
                (0..6)
                    .map(|_| AtomicUsize::new(0))
                    .collect(),
            ),
            prompt_sizes: Arc::new(std::sync::Mutex::new(Vec::new())),
            prompt_failures: Arc::new(AtomicUsize::new(0)),
            embedding_calls: Arc::new(AtomicUsize::new(0)),
            reference_calls: Arc::new(
                (0..6)
                    .map(|_| AtomicUsize::new(0))
                    .collect(),
            ),
            peak_references: Arc::new(AtomicUsize::new(0)),
        }
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
    mock.calls
        .fetch_add(1, Ordering::SeqCst);
    let malformed = mock.malformed.load(Ordering::SeqCst);
    if malformed == 7 {
        return (
            StatusCode::UNAUTHORIZED,
            Json(
                json!({"error":{"code":"invalid_api_key","message":"private provider diagnostic"}}),
            ),
        );
    }
    if malformed == 24 {
        return (
            StatusCode::OK,
            Json(
                json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"{\"unexpected\":true}"}]}]}),
            ),
        );
    }
    assert_eq!(input["model"], "gpt-5-mini");
    assert_eq!(input["store"], false);
    assert_eq!(input["text"]["format"]["strict"], true);
    let name = input["text"]["format"]["name"]
        .as_str()
        .unwrap();
    let request: Value = serde_json::from_str(
        input["input"][0]["content"]
            .as_str()
            .or_else(|| input["input"][0]["content"][0]["text"].as_str())
            .unwrap(),
    )
    .unwrap();
    let count = if name == "interview_blueprint" {
        request["question_count"]
            .as_u64()
            .unwrap() as usize
    } else {
        request["blueprint"]["slots"]
            .as_array()
            .map(Vec::len)
            .unwrap_or(1)
    };
    if !matches!(
        name,
        "interview_reference_answer" | "interview_reference_review" | "interview_prompt_review"
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
            let slots:Vec<Value>=(0..count).map(|i|json!({"competency_id":id,"kind":if i%2==0 {"text"}else{"scenario"},"difficulty":if i<count/3{"easy"}else if i<count-count/3{"medium"}else{"hard"}})).collect();
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
        "interview_questions" => {
            mock.prompt_calls
                .fetch_add(1, Ordering::SeqCst);
            mock.prompt_sizes
                .lock()
                .unwrap()
                .push(count);
            assert_eq!(count, 1);
            assert_eq!(
                input["text"]["format"]["schema"]["$defs"]["Question"]["properties"]["estimated_minutes"]
                    ["maximum"],
                request["duration_minutes"]
            );
            let slot = &request["blueprint"]["slots"][0];
            let index = request["positions"][0]
                .as_u64()
                .unwrap() as usize
                - 1;
            mock.reference_calls[index].fetch_add(1, Ordering::SeqCst);
            mock.peak_references
                .fetch_max(1, Ordering::SeqCst);
            let variant = mock.variant.load(Ordering::SeqCst);
            let attempt = request["draft_attempt"]
                .as_u64()
                .unwrap();
            let suffix = if attempt == 2 && matches!(malformed, 13 | 18 | 20 | 22) {
                " Clarify the transaction boundary and retry behavior; focus on a single state value."
            } else {
                ""
            };
            let bad = malformed == 3 || (malformed == 6 && index == 0 && attempt == 1);
            let mut reference = json!({"expected_answer":"Use an atomic transaction to commit the durable state together. Retry transient errors with bounded exponential backoff and jitter. Compare the consistency and availability tradeoffs, and make the operation idempotent to prevent duplicate side effects.","assumptions":["Assume a transactional PostgreSQL backend."],"acceptable_alternatives":["A transactional outbox is acceptable when delivery must occur outside the database transaction."],"common_mistakes":["Retrying a non-idempotent operation can apply a side effect twice."],"coding":null,"criteria":[{"id":"correctness","description":"Explains the relevant correctness guarantees clearly.","max_points":5,"partial_credit":"Identifies atomicity but omits failure handling details.","full_credit":"Explains how an atomic transaction preserves the durable state.","reference_answer":"Use an atomic transaction to commit the durable state together."},{"id":"tradeoffs","description":"Identifies concrete tradeoffs and failure handling.","max_points":if malformed==2 {4}else{5},"partial_credit":"Names a tradeoff without connecting it to this scenario.","full_credit":"Connects consistency and availability tradeoffs to idempotent retries.","reference_answer":if bad {"Missing"}else{"Compare the consistency and availability tradeoffs"}}]});
            assert_eq!(
                input["text"]["format"]["schema"]["$defs"]["Criterion"]["properties"]["id"]["pattern"],
                "^[a-z0-9_]{1,40}$"
            );
            if (index == 0 && (malformed == 15 || (malformed == 14 && attempt == 1)))
                || (malformed == 19 && index == 2)
            {
                reference["criteria"][0]["id"] = json!("Correctness-Score");
            }
            if malformed == 14 && index == 0 && attempt == 2 {
                assert!(
                    request["repair_feedback"]
                        .as_str()
                        .unwrap()
                        .contains("criterion 1 ID must")
                );
                assert!(request["previous_draft"].is_object());
            }
            if malformed == 10 {
                reference["unexpected_field"] = json!("Not in the required schema");
            }
            if malformed == 23 && index == 4 && attempt == 2 {
                reference["expected_answer"] = json!(
                    "This worked reference deliberately omits the required numeric validation. It does not detect malformed numeric parameters and therefore does not meet the stated input contract."
                );
            }
            let task = if (malformed == 12 && index == 1)
                || (malformed == 23 && index == 4 && attempt == 1)
            {
                0
            } else {
                index
            };
            let limit = request["variation"]["parameters"][0]["value"]
                .as_u64()
                .unwrap();
            let mut question = json!({"competency_id":slot["competency_id"],"kind":slot["kind"],"difficulty":slot["difficulty"],
                "prompt":format!("Variant {variant} task {task}: explain an appropriate reliability decision for a Rust service with PostgreSQL with a queue limit of {limit} requests.{suffix}"),
                "estimated_minutes":if malformed==5 {json!(30)}else{request["duration_minutes"].clone()}});
            question
                .as_object_mut()
                .unwrap()
                .extend(reference.as_object().unwrap().clone());
            assert_eq!(
                input["text"]["format"]["schema"]["$defs"]["Question"]["properties"]["coding"],
                json!({"type":"null"})
            );
            json!({"questions":[question]})
        }
        "interview_reference_review" => {
            let reference_reject = mock
                .review_failures
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok();
            let prompt = request["proposed_set"]["questions"][0]["prompt"]
                .as_str()
                .unwrap();
            let index = task_index(prompt);
            mock.prompt_reviews[index].fetch_add(1, Ordering::SeqCst);
            let prompt_reject = malformed == 13
                || (malformed == 18
                    && index == 2
                    && mock
                        .prompt_failures
                        .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                        .is_ok());
            let reject = reference_reject || prompt_reject;
            let factual_rejection = (malformed == 22
                && task_index(prompt) == 1
                && !prompt.contains("single state value"))
                || (malformed == 23
                    && task_index(prompt) == 4
                    && request["proposed_set"]["questions"][0]["expected_answer"]
                        .as_str()
                        .unwrap()
                        .contains("deliberately omits"));
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
                json!({"jd_grounded":!prompt_reject,"reference_correct":malformed!=4 && (!reject || malformed==20) && !factual_rejection,"rubric_aligned":true,"feasible":!(malformed==20 && reject),"unambiguous":true,"no_answer_leak":true,"concerns":if prompt_reject {vec!["Question needs concrete job-relevant inputs"]} else if factual_rejection {vec!["Reference invents an IAM action not established by the task"]} else if reject {if malformed==20 {vec!["Prompt must be simplified to fit five minutes"]}else{vec!["Explain rollback after a partial write"]}}else{vec![]},"advisories":if malformed==21 {vec!["Optional styling improvements are outside the prompt contract"]}else{vec![]}})
            }
        }
        _ => panic!("Unexpected generation stage: {name}"),
    };
    (
        StatusCode::OK,
        Json(
            json!({"id":"resp_mock","object":"response","created_at":0,"model":"gpt-5-mini","status":"completed","output":[{"type":"message","id":"msg_mock","role":"assistant","status":"completed","content":[{"type":"output_text","text":data.to_string()}]}]}),
        ),
    )
}
async fn embeddings(State(mock): State<Mock>, Json(input): Json<Value>) -> Json<Value> {
    mock.embedding_calls
        .fetch_add(1, Ordering::SeqCst);
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
            embedding[task_index(prompt)] = -1.0;
        } else {
            embedding[variant * 6 + task_index(prompt)] = 1.0;
        }
        data.push(json!({"object":"embedding","index":index,"embedding":embedding}));
    }
    // Deliberately reverse response order: the client must respect index.
    data.reverse();
    Json(
        json!({"object":"list","model":"text-embedding-3-small","usage":{"prompt_tokens":1,"total_tokens":1},"data":data}),
    )
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
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
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
async fn prepare_blueprint(pool: &PgPool, provider: &AiProvider, owner: Uuid, job: Uuid) {
    for _ in 0..64 {
        let mut tx = interviewer_transaction(pool, owner)
            .await
            .unwrap();
        let ready: bool = sqlx::query_scalar("SELECT blueprint IS NOT NULL FROM jobs WHERE id=$1")
            .bind(job)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        if ready {
            return;
        }
        assert!(
            process_next(pool, provider)
                .await
                .unwrap(),
            "blueprint queue stopped before this job was ready"
        );
    }
    panic!("blueprint did not complete");
}
/// A worker tick may claim a blueprint or another queued set first.
/// Stop after this set changes state; do not consume its automatic retries.
async fn process_candidate_attempt(
    pool: &PgPool,
    provider: &AiProvider,
    owner: Uuid,
    session: Uuid,
) {
    async fn state(pool: &PgPool, owner: Uuid, session: Uuid) -> (String, i32) {
        let mut tx = interviewer_transaction(pool, owner)
            .await
            .unwrap();
        let state = sqlx::query_as(
            "SELECT q.status, q.attempts FROM question_sets q \
             JOIN interview_sessions s ON s.question_set_id=q.id WHERE s.id=$1",
        )
        .bind(session)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        state
    }

    let initial = state(pool, owner, session).await;
    for _ in 0..64 {
        assert!(
            process_next(pool, provider)
                .await
                .unwrap(),
            "worker queue stopped before candidate {session} was processed"
        );
        if state(pool, owner, session).await != initial {
            return;
        }
    }
    panic!("candidate {session} was not processed within 64 worker ticks");
}

async fn schedule_candidate(
    pool: &PgPool,
    provider: &AiProvider,
    app: &Router,
    owner: Uuid,
    job: Uuid,
    token: &str,
) -> String {
    prepare_blueprint(pool, provider, owner, job).await;
    let (status, value) = call(
        app,
        "POST",
        &format!("/api/v1/jobs/{job}/invitations"),
        json!({
        "candidate_name":"Generation candidate", "candidate_email":"generation@example.test", "duration_minutes":30,
            "request_key":Uuid::new_v4()
        }),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{value}");
    assert!(value["invitation_token"].is_null());
    value["session"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}
async fn invitation(
    pool: &PgPool,
    owner: Uuid,
    app: &Router,
    base: &str,
    job: &str,
    token: &str,
) -> Value {
    let mut tx = interviewer_transaction(pool, owner)
        .await
        .unwrap();
    let session: Uuid = sqlx::query_scalar("SELECT id FROM interview_sessions WHERE job_id=$1")
        .bind(job.parse::<Uuid>().unwrap())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (status, value) = call(
        app,
        "POST",
        &format!("{base}/sessions/{session}/invitation"),
        json!({}),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    value
}
#[tokio::test]
#[ignore = "requires migrated PostgreSQL, restricted runtime role, and local mock HTTP listener"]
async fn generation_is_unique_recoverable_validated_and_private() {
    platform_api::infrastructure::load_environment().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&std::env::var("DATABASE_URL").unwrap())
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let service = Router::new()
        .route("/v1/responses", post(responses))
        .route("/v1/embeddings", post(embeddings))
        .with_state(mock.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, service)
            .await
            .unwrap()
    });
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
    let owner = account(&app).await;
    let outsider = account(&app).await;
    let token = owner["token"].as_str().unwrap();
    let user: Uuid = owner["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    for mode in [
        0, 2, 3, 4, 5, 6, 7, 10, 11, 12, 13, 14, 15, 18, 19, 20, 21, 22, 23, 24,
    ] {
        mock.malformed
            .store(0, Ordering::SeqCst);
        mock.variant
            .store(mode + 1, Ordering::SeqCst);
        let (_, job) = call(&app,"POST","/api/v1/jobs",json!({"job_code":format!("QUALITY-{}",Uuid::new_v4()),"title":"Reference quality","description":JD,"preparation_duration_minutes":30,"request_key":Uuid::new_v4()}),token).await;
        let job_id: Uuid = job["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let session = schedule_candidate(&pool, &provider, &app, user, job_id, token).await;
        mock.malformed
            .store(mode, Ordering::SeqCst);
        let route = format!("/api/v1/jobs/{job_id}/question-sets");
        assert_eq!(
            call(
                &app,
                "GET",
                &route,
                Value::Null,
                outsider["token"].as_str().unwrap()
            )
            .await
            .0,
            StatusCode::NOT_FOUND
        );
        let mut tx = interviewer_transaction(&pool, user)
            .await
            .unwrap();
        sqlx::query("UPDATE question_sets SET status='running',attempts=3,lease_until=now()-interval '1 second',claim_token=$2 WHERE job_id=$1")
            .bind(job_id).bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert!(
            process_next(&pool, &provider)
                .await
                .unwrap()
        );
        let (_, result) = call(&app, "GET", &route, Value::Null, token).await;
        assert_eq!(result["sets"][0]["attempts"], 4);
        let completed = matches!(mode, 0 | 6 | 8 | 9 | 11 | 14 | 18 | 20 | 21 | 22 | 23);
        assert_eq!(
            result["sets"][0]["status"],
            if completed { "completed" } else { "failed" },
            "mode {mode}: {result}"
        );
        if completed {
            assert_eq!(
                result["questions"]
                    .as_array()
                    .unwrap()
                    .len(),
                6
            );
            for q in result["questions"].as_array().unwrap() {
                assert_eq!(q["rubric"]["quality_review"]["position"], q["position"]);
                assert_eq!(q["rubric"]["quality_review"]["reference_correct"], true);
            }
            let candidate = invitation(
                &pool,
                user,
                &app,
                "/api/v1",
                job["id"].as_str().unwrap(),
                token,
            )
            .await;
            let candidate_token = candidate["invitation_token"]
                .as_str()
                .unwrap();
            let (_, view) = call(
                &app,
                "GET",
                "/api/v1/candidate/session",
                Value::Null,
                candidate_token,
            )
            .await;
            assert_eq!(view["questions_ready"], true);
            assert!(
                !view
                    .to_string()
                    .contains("expected_answer")
            );
            assert_eq!(
                call(&app, "GET", &route, Value::Null, candidate_token)
                    .await
                    .0,
                StatusCode::UNAUTHORIZED
            );
        } else {
            let saved = result["questions"].as_array().unwrap();
            assert_eq!(
                saved.len(),
                match mode {
                    12 => 1,
                    19 => 2,
                    _ => 0,
                },
                "mode {mode}"
            );
            assert_eq!(
                call(
                    &app,
                    "POST",
                    &format!("/api/v1/sessions/{session}/invitation"),
                    json!({}),
                    token
                )
                .await
                .0,
                StatusCode::CONFLICT
            );
            if matches!(mode, 12 | 19) {
                let ids: Vec<Value> = saved
                    .iter()
                    .map(|q| q["id"].clone())
                    .collect();
                mock.malformed
                    .store(0, Ordering::SeqCst);
                assert_eq!(
                    call(&app, "POST", &route, json!({}), token)
                        .await
                        .0,
                    StatusCode::ACCEPTED
                );
                assert!(
                    process_next(&pool, &provider)
                        .await
                        .unwrap()
                );
                let (_, resumed) = call(&app, "GET", &route, Value::Null, token).await;
                assert_eq!(resumed["sets"][0]["status"], "completed");
                for id in ids {
                    assert!(
                        resumed["questions"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|q| q["id"] == id)
                    );
                }
            }
        }
    }
    task.abort();
    let _ = task.await;
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires migrated PostgreSQL and local mock provider"]
async fn every_generation_failure_gets_three_worker_retries() {
    platform_api::infrastructure::load_environment().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    // Isolate this scheduling test from earlier synthetic jobs.
    let owners: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer'")
        .fetch_all(&pool)
        .await
        .unwrap();
    for owner in owners {
        let mut tx = interviewer_transaction(&pool, owner)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE question_sets SET available_at=now()+interval '1 day' WHERE status='pending'",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let mock = Mock::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let service = Router::new()
        .route("/v1/responses", post(responses))
        .route("/v1/embeddings", post(embeddings))
        .with_state(mock.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, service)
            .await
            .unwrap();
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
    let token = owner["token"].as_str().unwrap();
    let user: Uuid = owner["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    // Invalid credentials, unexpected payload, quality rejection and structural failure.
    for mode in [7, 24, 4, 19] {
        mock.malformed
            .store(0, Ordering::SeqCst);
        let (_,job)=call(&app,"POST","/api/v1/jobs",json!({"job_code":format!("RETRY-{}",Uuid::new_v4()),"title":"Retry policy","description":JD,"preparation_duration_minutes":30,"request_key":Uuid::new_v4()}),token).await;
        let job_id: Uuid = job["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        schedule_candidate(&pool, &provider, &app, user, job_id, token).await;
        mock.malformed
            .store(mode, Ordering::SeqCst);
        let route = format!("/api/v1/jobs/{job_id}/question-sets");
        let mut saved_ids = None;
        for attempt in 1..=4 {
            let mut tx = interviewer_transaction(&pool, user)
                .await
                .unwrap();
            sqlx::query("UPDATE question_sets SET available_at=now() WHERE job_id=$1")
                .bind(job_id)
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            assert!(
                process_next(&pool, &provider)
                    .await
                    .unwrap()
            );
            let (_, status) = call(&app, "GET", &route, Value::Null, token).await;
            assert_eq!(status["max_attempts"], 4);
            assert_eq!(status["sets"][0]["attempts"], attempt);
            assert_eq!(
                status["sets"][0]["status"],
                if attempt < 4 { "pending" } else { "failed" }
            );
            if mode == 19 {
                let ids: Vec<Value> = status["questions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|q| q["id"].clone())
                    .collect();
                assert_eq!(ids.len(), 2);
                if let Some(previous) = &saved_ids {
                    assert_eq!(&ids, previous);
                } else {
                    saved_ids = Some(ids);
                }
            }
            // There is no fifth attempt, and pending retries respect their delay.
            assert!(
                !process_next(&pool, &provider)
                    .await
                    .unwrap()
            );
        }
    }
    server.abort();
    let _ = server.await;
    pool.close().await;
}

/// Use a disposable database and local mock only, never the live AI provider.
#[tokio::test]
#[ignore = "requires migrated DATABASE_URL, invitation key and local mock listener"]
async fn candidate_invitation_waits_for_its_own_questions() {
    platform_api::infrastructure::load_environment().unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("DATABASE_URL").expect("use a disposable test database"))
        .await
        .unwrap();
    let mock = Mock::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let service = Router::new()
        .route("/v1/responses", post(responses))
        .route("/v1/embeddings", post(embeddings))
        .with_state(mock.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, service)
            .await
            .unwrap();
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
    let token = owner["token"].as_str().unwrap();
    let user: Uuid = owner["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let (_, job) = call(
        &app,
        "POST",
        "/api/v1/jobs",
        json!({
            "job_code":format!("PRIVATE-{}",Uuid::new_v4()), "title":"Rust engineer",
            "description":JD,"preparation_duration_minutes":10,"request_key":Uuid::new_v4()
        }),
        token,
    )
    .await;
    let job_id = job["id"].as_str().unwrap();
    let mut tx = interviewer_transaction(&pool, user)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM question_sets WHERE job_id=$1")
        .bind(job_id.parse::<Uuid>().unwrap())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(count, 0, "job creation must not generate reusable sets");
    let blueprint_ready: bool =
        sqlx::query_scalar("SELECT blueprint IS NOT NULL FROM jobs WHERE id=$1")
            .bind(job_id.parse::<Uuid>().unwrap())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(
        !blueprint_ready,
        "job creation queues blueprint work without blocking"
    );
    tx.commit().await.unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/jobs/{job_id}/invitations"),
            json!({
                "candidate_name":"Waiting candidate", "candidate_email":"waiting@example.test",
                "duration_minutes":10, "request_key":Uuid::new_v4()
            }),
            token
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "invitations wait for the background blueprint"
    );
    prepare_blueprint(&pool, &provider, user, job_id.parse().unwrap()).await;
    let mut tx = interviewer_transaction(&pool, user)
        .await
        .unwrap();
    let blueprint_ready: bool =
        sqlx::query_scalar("SELECT blueprint IS NOT NULL FROM jobs WHERE id=$1")
            .bind(job_id.parse::<Uuid>().unwrap())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(
        blueprint_ready,
        "the worker must save the blueprint before invitations"
    );
    tx.commit().await.unwrap();
    let route = format!("/api/v1/jobs/{job_id}/invitations");
    let (status, _) = call(
        &app,
        "POST",
        &route,
        json!({
            "candidate_name":"Invalid duration", "candidate_email":"invalid@example.test",
            "duration_minutes":30,"request_key":Uuid::new_v4()
        }),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let input = json!({"candidate_name":"First candidate", "candidate_email":"first@example.test",
        "duration_minutes":10,"request_key":Uuid::new_v4()});
    let (status, first) = call(&app, "POST", &route, input.clone(), token).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(first["invitation_token"].is_null());
    let (_, replay) = call(&app, "POST", &route, input, token).await;
    assert_eq!(first["session"]["id"], replay["session"]["id"]);
    let session = first["session"]["id"].as_str().unwrap();
    let link_route = format!("/api/v1/sessions/{session}/invitation");
    assert_eq!(
        call(&app, "POST", &link_route, json!({}), token)
            .await
            .0,
        StatusCode::CONFLICT
    );
    process_candidate_attempt(&pool, &provider, user, session.parse().unwrap()).await;
    let (_, progress) = call(
        &app,
        "GET",
        &format!("/api/v1/sessions/{session}/generation"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(progress["status"], "completed", "{progress}");
    let (status, link) = call(&app, "POST", &link_route, json!({}), token).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !link["invitation_token"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    let (_, same_link) = call(&app, "POST", &link_route, json!({}), token).await;
    assert_eq!(link["invitation_token"], same_link["invitation_token"]);
    let (_, second) = call(&app, "POST", &route, json!({"candidate_name":"Second candidate",
        "candidate_email":"second@example.test","duration_minutes":10,"request_key":Uuid::new_v4()}), token).await;
    assert!(second["invitation_token"].is_null());
    assert_ne!(
        first["session"]["question_set_id"],
        second["session"]["question_set_id"]
    );
    let second_id = second["session"]["id"]
        .as_str()
        .unwrap();
    let second_uuid: Uuid = second_id.parse().unwrap();
    let mut tx = interviewer_transaction(&pool, user)
        .await
        .unwrap();
    let waiting: bool = sqlx::query_scalar("SELECT expires_at='infinity'::timestamptz AND NOT EXISTS (SELECT 1 FROM auth_sessions a WHERE a.interview_session_id=s.id) FROM interview_sessions s WHERE id=$1")
        .bind(second_uuid).fetch_one(&mut *tx).await.unwrap();
    assert!(
        waiting,
        "preparation must have neither a token nor a running expiry"
    );
    sqlx::query("UPDATE question_sets SET attempts=3 WHERE id=(SELECT question_set_id FROM interview_sessions WHERE id=$1)")
        .bind(second_uuid).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    mock.malformed
        .store(7, Ordering::SeqCst);
    process_candidate_attempt(&pool, &provider, user, second_uuid).await;
    let generation_route = format!("/api/v1/sessions/{second_id}/generation");
    let (_, failed) = call(&app, "GET", &generation_route, Value::Null, token).await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/sessions/{second_id}/invitation"),
            json!({}),
            token
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&app, "POST", &generation_route, json!({}), token)
            .await
            .0,
        StatusCode::ACCEPTED
    );
    mock.malformed
        .store(0, Ordering::SeqCst);
    mock.variant.store(4, Ordering::SeqCst);
    process_candidate_attempt(&pool, &provider, user, second_uuid).await;
    let second_id = second["session"]["id"]
        .as_str()
        .unwrap();
    let (_, second_progress) = call(
        &app,
        "GET",
        &format!("/api/v1/sessions/{second_id}/generation"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(second_progress["status"], "completed", "{second_progress}");
    assert_ne!(
        progress["questions"][0]["prompt"],
        second_progress["questions"][0]["prompt"]
    );
    server.abort();
}
