use axum::{
    Json, Router,
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

async fn mock_embeddings(Json(body): Json<Value>) -> Json<Value> {
    let data: Vec<Value> = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let mut embedding = vec![0.0f32; 1536];
            embedding[0] = 1.0;
            json!({"object":"embedding","index":index,"embedding":embedding})
        })
        .collect();
    Json(
        json!({"object":"list","model":"text-embedding-3-small","usage":{"prompt_tokens":1,"total_tokens":1},"data":data}),
    )
}
async fn mock_response(Json(body): Json<Value>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let input: Value = serde_json::from_str(
        body["input"][0]["content"]
            .as_str()
            .or_else(|| body["input"][0]["content"][0]["text"].as_str())
            .unwrap(),
    )
    .unwrap();
    if body["stream"] == true {
        let source = &input["sources"][0];
        let quote: String = source["text"]
            .as_str()
            .unwrap()
            .chars()
            .take(30)
            .collect();
        let reply=json!({"answer":"Evidence-based session summary.","citations":[{"source_id":source["id"],"quote":quote}]}).to_string();
        let mut data = String::new();
        for character in reply.chars() {
            data.push_str(&format!(
                "event: response.output_text.delta\ndata: {}\n\n",
                json!({"type":"response.output_text.delta","item_id":"msg_mock","output_index":0,"content_index":0,"sequence_number":0,"delta":character.to_string()})
            ));
        }
        data.push_str(&format!(
            "data: {}\n\n",
            json!({"type":"response.completed","sequence_number":1,"response":{"id":"resp_mock","object":"response","created_at":0,"model":"gpt-5-mini","status":"completed","output":[]}})
        ));
        ([("content-type", "text/event-stream")], data).into_response()
    } else {
        Json(json!({"id":"resp_mock","object":"response","created_at":0,"model":"gpt-5-mini","status":"completed","output":[{"type":"message","id":"msg_mock","role":"assistant","status":"completed","content":[{"type":"output_text","text":json!({"criteria":[{"criterion_id":"correctness","points":9,"evidence":"reliable","reasoning":"The answer explicitly describes a reliable approach."}]}).to_string()}]}]})).into_response()
    }
}
#[tokio::test]
#[ignore = "requires PostgreSQL and localhost mock AI"]
async fn reviewer_stream_and_adaptive_selection_are_scoped_and_grounded() {
    platform_api::infrastructure::load_environment().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/responses", axum::routing::post(mock_response))
                .route("/embeddings", axum::routing::post(mock_embeddings)),
        )
        .await
        .unwrap();
    });
    // This test binary has one test; configure its provider before any request tasks start.
    unsafe {
        std::env::set_var("LLM_BASE_URL", format!("http://{address}"));
        std::env::set_var("LLM_API_KEY", "mock-only");
        std::env::set_var("EMBEDDING_API_KEY", "mock-only");
    }
    let pool = PgPoolOptions::new()
        .connect(&std::env::var("RUNTIME_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let app = platform_api::router_with_ai(pool.clone(), None);
    let owner = account(&app).await;
    let (invite, questions) = setup(&pool, &app, &owner).await;
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
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/v1/candidate/answers/{}", questions[0]),
            json!({"content":"A reliable solution with tradeoffs","revision":0}),
            candidate
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, next) = call(
        &app,
        "POST",
        "/api/v1/candidate/recommendation",
        json!({"question_id":questions[0]}),
        candidate,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(next["position"], 2);
    assert!(next.get("provisional_grade").is_none());
    // An invalid provisional AI quote must not block the Next button.
    let (status, _) = call(
        &app,
        "POST",
        &format!("/api/v1/candidate/answers/{}", questions[0]),
        json!({"content":"Borrowing avoids copying","revision":1}),
        candidate,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, next) = call(
        &app,
        "POST",
        "/api/v1/candidate/recommendation",
        json!({"question_id":questions[0]}),
        candidate,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(next["position"], 2);
    assert!(next.get("provisional_grade").is_none());
    // Skipping an unanswered last question does not need AI configuration/grading.
    let (status, next) = call(
        &app,
        "POST",
        "/api/v1/candidate/recommendation",
        json!({"question_id":questions[1]}),
        candidate,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(next["position"].is_null());
    let response=app.clone().oneshot(Request::builder().method("POST").uri(format!("/api/v1/sessions/{session}/review")).header("authorization",format!("Bearer {}",owner["token"].as_str().unwrap())).header("content-type","application/json").body(Body::from(json!({"question":"What does the evidence show?","conversation":Uuid::new_v4(),"page":0}).to_string())).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        to_bytes(response.into_body(), 262144),
    )
    .await
    .unwrap()
    .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: delta"));
    assert!(body.contains("event: complete"));
    assert!(!body.contains("event: failure"));
    let outsider = account(&app).await;
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/sessions/{session}/live"),
            Value::Null,
            outsider["token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    server.abort();
}
