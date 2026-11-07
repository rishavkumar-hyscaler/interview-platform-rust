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
            let mut question = json!({"competency_id":slot["competency_id"],"kind":slot["kind"],"difficulty":slot["difficulty"],
                "prompt":format!("Variant {variant} task {task}: explain an appropriate reliability decision for a Rust service with PostgreSQL under these constraints.{suffix}"),
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
async fn invitation(
    _pool: &PgPool,
    _owner: Uuid,
    app: &Router,
    base: &str,
    job: &str,
    token: &str,
) -> Value {
    let (status,value)=call(app,"POST",&format!("{base}/jobs/{job}/invitations"),json!({"candidate_name":"Generation candidate","candidate_email":"generation@example.test","duration_minutes":30,"request_key":Uuid::new_v4()}),token).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "only completed job sets can be invited: {value}"
    );
    value
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
    for mode in 0..=24 {
        mock.malformed
            .store(mode, Ordering::SeqCst);
        mock.variant
            .store(mode + 1, Ordering::SeqCst);
        let (_, job) = call(&app,"POST","/api/v1/jobs",json!({"job_code":format!("QUALITY-{}",Uuid::new_v4()),"title":"Reference quality","description":JD,"question_set_count":1,"request_key":Uuid::new_v4()}),token).await;
        let job_id: Uuid = job["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
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
            assert_eq!(call(&app,"POST",&format!("/api/v1/jobs/{job_id}/invitations"),json!({"candidate_name":"Blocked candidate","candidate_email":"blocked@example.test","duration_minutes":30,"request_key":Uuid::new_v4()}),token).await.0,StatusCode::CONFLICT);
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
#[ignore = "requires migrated PostgreSQL, restricted runtime role, and local mock HTTP listener"]
async fn job_sets_prepare_once_and_are_reused_without_replenishment() {
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
    let task = tokio::spawn(async move {
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
    let other = account(&app).await;
    let token = owner["token"].as_str().unwrap();
    let recruiter: Uuid = owner["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let (_, older_job) = call(&app,"POST","/api/v1/jobs",json!({"job_code":format!("OLDER-{}",Uuid::new_v4()),"title":"Older queued role","description":JD,"request_key":Uuid::new_v4()}),token).await;
    assert_eq!(older_job["question_set_count"], 1);
    let (_, default_pool) = call(
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
    assert_eq!(default_pool["target"], 1);
    assert_eq!(
        default_pool["sets"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for invalid_count in [0, 6] {
        let (status, _) = call(&app, "POST", "/api/v1/jobs", json!({"job_code":format!("INVALID-{}",Uuid::new_v4()),"title":"Invalid pool","description":JD,"question_set_count":invalid_count,"request_key":Uuid::new_v4()}), token).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    let input = json!({"job_code":format!("POOL-{}",Uuid::new_v4()),"title":"Prepared Rust role","description":JD,"question_set_count":3,"preparation_duration_minutes":30,"request_key":Uuid::new_v4()});
    let (status, job) = call(&app, "POST", "/api/v1/jobs", input.clone(), token).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        call(&app, "POST", "/api/v1/jobs", input, token)
            .await
            .0,
        StatusCode::OK
    );
    let id = job["id"].as_str().unwrap();
    assert_eq!(job["question_set_count"], 3);
    let route = format!("/api/v1/jobs/{id}/question-sets");
    let (_, initial) = call(&app, "GET", &route, Value::Null, token).await;
    assert_eq!(
        initial["sets"]
            .as_array()
            .unwrap()
            .len(),
        3,
        "idempotent job creation queues exactly three sets"
    );
    assert!(
        initial["questions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
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
    let first: Uuid = initial["sets"][0]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    sqlx::query("UPDATE question_sets SET status='running',attempts=1,claim_token=$2,lease_until=now()-interval '1 second' WHERE id=$1").bind(first).bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    for variant in [1, 2] {
        mock.variant
            .store(variant, Ordering::SeqCst);
        assert!(
            process_next(&pool, &provider)
                .await
                .unwrap()
        );
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
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    sqlx::query("UPDATE question_sets SET available_at=now()+interval '1 day' WHERE job_id=$1")
        .bind(
            older_job["id"]
                .as_str()
                .unwrap()
                .parse::<Uuid>()
                .unwrap(),
        )
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    mock.variant.store(4, Ordering::SeqCst);
    assert!(
        process_next(&pool, &provider)
            .await
            .unwrap()
    );
    let (_, ready) = call(&app, "GET", &route, Value::Null, token).await;
    assert_eq!(
        ready["questions"]
            .as_array()
            .unwrap()
            .len(),
        18
    );
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
        invitation(&pool, recruiter, &app, "/api/v1", id, token),
        invitation(&pool, recruiter, &app, "/api/v1", id, token)
    );
    assert_eq!(a["session"]["question_set_id"].is_string(), true);
    assert_eq!(b["session"]["question_set_id"].is_string(), true);
    assert_ne!(
        a["session"]["question_set_id"],
        b["session"]["question_set_id"]
    );
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
    assert!(
        !view
            .to_string()
            .contains("expected_answer")
    );
    assert_eq!(
        call(&app, "GET", &route, Value::Null, candidate)
            .await
            .0,
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
        remaining["questions"]
            .as_array()
            .unwrap()
            .len(),
        18,
        "reusable source questions remain available in pool preview"
    );
    let unconfigured = platform_api::router_with_ai(pool.clone(), None);
    let third = invitation(&pool, recruiter, &unconfigured, "/api/v1", id, token).await;
    assert_eq!(
        third["session"]["question_set_id"].is_string(),
        true,
        "existing sets work even with provider offline"
    );
    let fourth = invitation(&pool, recruiter, &app, "/api/v1", id, token).await;
    assert_eq!(
        fourth["session"]["question_set_id"].is_string(),
        true,
        "the fourth candidate reuses one of the three prepared sets"
    );
    let (_, exhausted) = call(&app, "GET", &route, Value::Null, token).await;
    assert_eq!(
        exhausted["sets"]
            .as_array()
            .unwrap()
            .len(),
        3,
        "assignment never replenishes sets"
    );
    let (_,diagnostic_job)=call(&app,"POST","/api/v1/jobs",json!({"job_code":format!("DIAGNOSTIC-{}",Uuid::new_v4()),"title":"Failure diagnostics","description":JD,"request_key":Uuid::new_v4()}),token).await;
    let id = diagnostic_job["id"].as_str().unwrap();
    let route = format!("/api/v1/jobs/{id}/question-sets");
    // Failed jobs remain bounded until an authorized explicit retry.
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    sqlx::query("UPDATE jobs SET blueprint=$2 WHERE id=$1")
        .bind(id.parse::<Uuid>().unwrap())
        .bind(&ready["blueprint"])
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    sqlx::query("UPDATE question_sets SET status='failed',attempts=3,last_error='Synthetic quality failure' WHERE job_id=$1").bind(id.parse::<Uuid>().unwrap()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    sqlx::query("UPDATE question_sets SET status='pending',attempts=3,available_at=now() WHERE id=(SELECT id FROM question_sets WHERE job_id=$1 ORDER BY id LIMIT 1)")
        .bind(id.parse::<Uuid>().unwrap()).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    mock.malformed
        .store(7, Ordering::SeqCst);
    assert!(
        process_next(&pool, &provider)
            .await
            .unwrap()
    );
    let (_, failure) = call(&app, "GET", &route, Value::Null, token).await;
    let diagnostic = failure["sets"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|set| set["last_failure"].as_object());
    let diagnostic = diagnostic.expect("owner should see a saved failure diagnostic");
    assert_eq!(diagnostic["phase"], "question");
    assert!(!diagnostic.contains_key("draft"));
    assert!(
        failure["sets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|set| set["status"] == "failed" && set["attempts"] == 4)
    );
    assert_eq!(
        call(&unconfigured, "POST", &route, json!({}), token)
            .await
            .0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        call(&app, "POST", &route, json!({}), token)
            .await
            .0,
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
    // A pool set also preserves completed questions and cannot be assigned early.
    mock.malformed
        .store(19, Ordering::SeqCst);
    mock.variant.store(1, Ordering::SeqCst);
    let (_,checkpoint_job)=call(&app,"POST","/api/v1/jobs",json!({"job_code":format!("CHECKPOINT-{}",Uuid::new_v4()),"title":"Checkpoint pool role","description":JD,"request_key":Uuid::new_v4()}),token).await;
    let checkpoint_id = checkpoint_job["id"].as_str().unwrap();
    let checkpoint_route = format!("/api/v1/jobs/{checkpoint_id}/question-sets");
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    sqlx::query("UPDATE question_sets SET available_at=now()+interval '1 day' WHERE job_id<>$1 AND status='pending'").bind(checkpoint_id.parse::<Uuid>().unwrap()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE question_sets SET attempts=3 WHERE job_id=$1")
        .bind(checkpoint_id.parse::<Uuid>().unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        process_next(&pool, &provider)
            .await
            .unwrap()
    );
    let (_, partial) = call(&app, "GET", &checkpoint_route, json!({}), token).await;
    let partial_set = partial["sets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|set| set["status"] == "failed")
        .unwrap();
    let partial_id = partial_set["id"].as_str().unwrap();
    assert_eq!(partial_set["question_count"], 2);
    assert_eq!(
        partial["questions"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "interviewer should see saved progress for an incomplete set"
    );
    let saved_ids: Vec<Value> = partial["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|question| question["id"].clone())
        .collect();
    assert_eq!(call(&app,"POST",&format!("/api/v1/jobs/{checkpoint_id}/invitations"),json!({"candidate_name":"Blocked candidate","candidate_email":"blocked@example.test","duration_minutes":30,"request_key":Uuid::new_v4()}),token).await.0,StatusCode::CONFLICT,"incomplete sets cannot be assigned");
    let before: Vec<usize> = mock
        .reference_calls
        .iter()
        .map(|count| count.load(Ordering::SeqCst))
        .collect();
    mock.malformed
        .store(0, Ordering::SeqCst);
    assert_eq!(
        call(&app, "POST", &checkpoint_route, json!({}), token)
            .await
            .0,
        StatusCode::ACCEPTED
    );
    assert!(
        process_next(&pool, &provider)
            .await
            .unwrap()
    );
    let (_, resumed) = call(&app, "GET", &checkpoint_route, json!({}), token).await;
    let completed = resumed["sets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|set| set["id"] == partial_id)
        .unwrap();
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["question_count"], 6);
    for saved in saved_ids {
        assert!(
            resumed["questions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|question| question["id"] == saved)
        );
    }
    for (index, (counter, before)) in mock
        .reference_calls
        .iter()
        .zip(before)
        .enumerate()
    {
        assert_eq!(
            counter.load(Ordering::SeqCst) - before,
            if index < 2 { 0 } else { 1 },
            "pool retries must skip every saved question"
        );
    }
    // A one-set job serves the same content to every candidate with independent answers and no question copies.
    let calls_before_reuse = mock.calls.load(Ordering::SeqCst);
    let first_candidate = invitation(&pool, recruiter, &app, "/api/v1", checkpoint_id, token).await;
    let second_candidate =
        invitation(&pool, recruiter, &app, "/api/v1", checkpoint_id, token).await;
    assert_eq!(
        first_candidate["session"]["question_set_id"],
        second_candidate["session"]["question_set_id"]
    );
    assert_eq!(mock.calls.load(Ordering::SeqCst), calls_before_reuse);
    let first_session: Uuid = first_candidate["session"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let second_session: Uuid = second_candidate["session"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    let shared: i64 = sqlx::query_scalar("SELECT count(*) FROM session_question_details() a JOIN session_question_details() b ON a.id=b.id WHERE a.session_id=$1 AND b.session_id=$2 AND a.prompt=b.prompt AND a.rubric=b.rubric")
        .bind(first_session).bind(second_session).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(shared, 6);
    let stored: i64 =
        sqlx::query_scalar("SELECT count(*) FROM interview_questions WHERE job_id=$1")
            .bind(checkpoint_id.parse::<Uuid>().unwrap())
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(stored, 6, "assigning candidates does not copy questions");
    let sets: i64 = sqlx::query_scalar("SELECT count(*) FROM question_sets WHERE job_id=$1")
        .bind(checkpoint_id.parse::<Uuid>().unwrap())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(sets, 1);
    tx.commit().await.unwrap();
    let first_token = first_candidate["invitation_token"]
        .as_str()
        .unwrap();
    let second_token = second_candidate["invitation_token"]
        .as_str()
        .unwrap();
    for candidate_token in [first_token, second_token] {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/candidate/session/ready",
                json!({}),
                candidate_token
            )
            .await
            .0,
            StatusCode::OK
        );
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/candidate/session/start",
                json!({}),
                candidate_token
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let (_, first_room) = call(
        &app,
        "GET",
        "/api/v1/candidate/room?position=1",
        Value::Null,
        first_token,
    )
    .await;
    let shared_question = first_room["question"]["id"]
        .as_str()
        .unwrap();
    let answer_route = format!("/api/v1/candidate/answers/{shared_question}");
    assert_eq!(
        call(
            &app,
            "POST",
            &answer_route,
            json!({"content":"Candidate one independent answer","revision":0}),
            first_token
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, second_room) = call(
        &app,
        "GET",
        "/api/v1/candidate/room?position=1",
        Value::Null,
        second_token,
    )
    .await;
    assert_eq!(second_room["question"]["id"], shared_question);
    assert_eq!(second_room["question"]["answer"], "");
    assert_eq!(second_room["session"]["answer_count"], 0);
    assert_eq!(
        call(
            &app,
            "POST",
            &answer_route,
            json!({"content":"Candidate two independent answer","revision":0}),
            second_token
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, first_again) = call(
        &app,
        "GET",
        "/api/v1/candidate/room?position=1",
        Value::Null,
        first_token,
    )
    .await;
    assert_eq!(
        first_again["question"]["answer"],
        "Candidate one independent answer"
    );
    assert_eq!(first_again["question"]["revision"], 1);
    // Plan a short interview before drafting and keep exactly that plan on retry.
    mock.malformed
        .store(19, Ordering::SeqCst);
    let (_,short_job)=call(&app,"POST","/api/v1/jobs",json!({"job_code":format!("SHORT-{}",Uuid::new_v4()),"title":"Short focused role","description":JD,"preparation_duration_minutes":10,"question_set_count":1,"request_key":Uuid::new_v4()}),token).await;
    let short_id = short_job["id"].as_str().unwrap();
    let short_route = format!("/api/v1/jobs/{short_id}/question-sets");
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    sqlx::query("UPDATE question_sets SET available_at=now()+interval '1 day' WHERE job_id<>$1 AND status='pending'").bind(short_id.parse::<Uuid>().unwrap()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE question_sets SET attempts=3 WHERE job_id=$1")
        .bind(short_id.parse::<Uuid>().unwrap())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        process_next(&pool, &provider)
            .await
            .unwrap()
    );
    let (_, short_partial) = call(&app, "GET", &short_route, Value::Null, token).await;
    let time_plan = short_partial["sets"][0]["plan"].clone();
    assert_eq!(short_partial["sets"][0]["question_count"], 2);
    assert_eq!(
        time_plan["allocations"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert!(
        time_plan["allocations"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["minutes"] == 2)
    );
    mock.malformed
        .store(0, Ordering::SeqCst);
    assert_eq!(
        call(&app, "POST", &short_route, json!({}), token)
            .await
            .0,
        StatusCode::ACCEPTED
    );
    assert!(
        process_next(&pool, &provider)
            .await
            .unwrap()
    );
    let (_, short_complete) = call(&app, "GET", &short_route, Value::Null, token).await;
    assert_eq!(short_complete["sets"][0]["status"], "completed");
    assert_eq!(short_complete["sets"][0]["plan"], time_plan);
    assert_eq!(
        short_complete["questions"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert!(
        short_complete["questions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|q| q["rubric"]["estimated_minutes"] == 2)
    );
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
        .connect(
            &std::env::var("RUNTIME_DATABASE_URL")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .unwrap(),
        )
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
            .store(mode, Ordering::SeqCst);
        let (_,job)=call(&app,"POST","/api/v1/jobs",json!({"job_code":format!("RETRY-{}",Uuid::new_v4()),"title":"Retry policy","description":JD,"request_key":Uuid::new_v4()}),token).await;
        let job_id: Uuid = job["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
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
