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
    let bytes = to_bytes(response.into_body(), 262144).await.unwrap();
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
    let user: Uuid = owner["user_id"].as_str().unwrap().parse().unwrap();
    let(s,job)=call(app,"POST","/api/v1/jobs",json!({"job_code":format!("TEST-{}",Uuid::new_v4()),"title":"Interview room test","description":"Build reliable applications in Rust and PostgreSQL.","request_key":Uuid::new_v4()}),token).await;
    assert_eq!(s, StatusCode::CREATED);
    let(s,invite)=call(app,"POST",&format!("/api/v1/jobs/{}/invitations",job["id"].as_str().unwrap()),json!({"candidate_name":"Room test candidate","candidate_email":"room-test@example.test","duration_minutes":30,"request_key":Uuid::new_v4()}),token).await;
    assert_eq!(s, StatusCode::CREATED);
    let id: Uuid = invite["session"]["id"].as_str().unwrap().parse().unwrap();
    let job: Uuid = job["id"].as_str().unwrap().parse().unwrap();
    let ids = vec![Uuid::new_v4(), Uuid::new_v4()];
    let mut tx = interviewer_transaction(pool, user).await.unwrap();
    for (i, q) in ids.iter().enumerate() {
        sqlx::query("INSERT INTO interview_questions(id,interviewer_id,session_id,job_id,fingerprint,position,kind,difficulty,prompt,rubric) VALUES($1,$2,$3,$4,$5,$6,$7,'medium','Explain a reliable solution with clear tradeoffs','{\"criteria\":[{\"id\":\"correctness\",\"max_points\":10}],\"max_score\":10}')").bind(q).bind(user).bind(id).bind(job).bind(q.to_string()).bind((i+1)as i32).bind(if i==0{"text"}else{"code"}).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    (invite, ids)
}

async fn binary(
    app: &Router,
    method: &str,
    path: &str,
    mime: &str,
    bytes: Vec<u8>,
    token: &str,
) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("authorization", format!("Bearer {token}"))
                .header("content-type", mime)
                .body(Body::from(bytes))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
        .await
        .unwrap();
    (status, bytes.to_vec())
}
struct Cleanup(std::path::PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[tokio::test]
#[ignore = "requires migrated PostgreSQL and private temporary media storage"]
async fn recordings_are_private_bounded_retry_safe_and_not_falsely_zero_scored() {
    platform_api::infrastructure::load_environment().unwrap();
    let dir = std::env::temp_dir().join(format!("interview-media-test-{}", Uuid::new_v4()));
    let _cleanup = Cleanup(dir.clone());
    unsafe {
        std::env::set_var("MEDIA_STORAGE_DIR", &dir);
    }
    let pool = PgPoolOptions::new()
        .connect(&std::env::var("RUNTIME_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let app = platform_api::router_with_ai(pool.clone(), None);
    let owner = account(&app).await;
    let (invite, questions) = setup(&pool, &app, &owner).await;
    let (other, other_questions) = setup(&pool, &app, &owner).await;
    let session: Uuid = invite["session"]["id"].as_str().unwrap().parse().unwrap();
    let owner_id: Uuid = owner["user_id"].as_str().unwrap().parse().unwrap();
    let candidate = invite["invitation_token"].as_str().unwrap();
    let mut tx = interviewer_transaction(&pool, owner_id).await.unwrap();
    sqlx::query("UPDATE interview_sessions SET state='in_progress',started_at=now(),deadline_at=now()+interval '30 minutes' WHERE id=$1").bind(session).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let id = Uuid::new_v4();
    let path = format!(
        "/api/v1/candidate/questions/{}/recordings/{id}",
        questions[0]
    );
    let mut bytes = vec![0; 600000];
    bytes[..4].copy_from_slice(&[0x1a, 0x45, 0xdf, 0xa3]);
    // Simulate an incomplete file from a cancelled request, without a committed audit row.
    let orphan = dir
        .join(owner_id.to_string())
        .join(session.to_string())
        .join(id.to_string());
    tokio::fs::create_dir_all(orphan.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&orphan, b"partial upload").await.unwrap();
    assert_eq!(
        binary(&app, "POST", &path, "audio/webm", bytes.clone(), candidate)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        binary(&app, "POST", &path, "audio/webm", bytes.clone(), candidate)
            .await
            .0,
        StatusCode::OK
    );
    let mut changed = bytes.clone();
    changed[100] = 1;
    assert_eq!(
        binary(&app, "POST", &path, "audio/webm", changed, candidate)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        binary(
            &app,
            "POST",
            &format!(
                "/api/v1/candidate/questions/{}/recordings/{}",
                other_questions[0],
                Uuid::new_v4()
            ),
            "audio/webm",
            bytes.clone(),
            candidate
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let (candidate_status, received) = binary(
        &app,
        "GET",
        &format!("/api/v1/candidate/recordings/{id}"),
        "",
        vec![],
        candidate,
    )
    .await;
    assert_eq!(candidate_status, StatusCode::OK);
    assert_eq!(received, bytes);
    assert_eq!(
        binary(
            &app,
            "GET",
            &format!("/api/v1/candidate/recordings/{id}"),
            "",
            vec![],
            other["invitation_token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let staff_path = format!("/api/v1/sessions/{session}/recordings/{id}");
    assert_eq!(
        binary(
            &app,
            "GET",
            &staff_path,
            "",
            vec![],
            owner["token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::OK
    );
    let outsider = account(&app).await;
    assert_eq!(
        binary(
            &app,
            "GET",
            &staff_path,
            "",
            vec![],
            outsider["token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let file = dir
        .join(owner_id.to_string())
        .join(session.to_string())
        .join(id.to_string());
    tokio::fs::write(&file, b"modified").await.unwrap();
    assert_eq!(
        binary(
            &app,
            "GET",
            &staff_path,
            "",
            vec![],
            owner["token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    tokio::fs::write(&file, &bytes).await.unwrap();
    let (_, view) = call(
        &app,
        "GET",
        "/api/v1/candidate/session",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(view["answer_count"], 1);
    let mut oversized = vec![0; 15 * 1024 * 1024 + 1];
    oversized[..4].copy_from_slice(&bytes[..4]);
    assert_eq!(
        binary(
            &app,
            "POST",
            &format!(
                "/api/v1/candidate/questions/{}/recordings/{}",
                questions[0],
                Uuid::new_v4()
            ),
            "audio/webm",
            oversized,
            candidate
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let mut tx = interviewer_transaction(&pool, owner_id).await.unwrap();
    sqlx::query("UPDATE interview_sessions SET deadline_at=now()-interval '1 second' WHERE id=$1")
        .bind(session)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        binary(
            &app,
            "POST",
            &format!(
                "/api/v1/candidate/questions/{}/recordings/{}",
                questions[0],
                Uuid::new_v4()
            ),
            "audio/webm",
            bytes.clone(),
            candidate
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        binary(&app, "POST", &path, "audio/webm", bytes, candidate)
            .await
            .0,
        StatusCode::OK
    );
    let provider = platform_api::generation::provider::AiProvider::new(
        "http://127.0.0.1:1".into(),
        "mock".into(),
        "mock".into(),
        "mock".into(),
        "mock".into(),
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
    let (_, report) = call(
        &app,
        "GET",
        &format!("/api/v1/sessions/{session}/assessment"),
        Value::Null,
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert!(report["assessment"]["questions"][0]["score"].is_null());
    assert_eq!(
        report["assessment"]["questions"][0]["status"],
        "media_requires_human_review"
    );
}
