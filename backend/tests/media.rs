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
    let job: Uuid = job["id"].as_str().unwrap().parse().unwrap();
    let ids = vec![Uuid::new_v4(), Uuid::new_v4()];
    let mut tx = interviewer_transaction(pool, user).await.unwrap();
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

#[tokio::test]
#[ignore = "requires migrated PostgreSQL and private temporary evidence storage"]
async fn proctor_evidence_is_linked_private_idempotent_and_excluded_from_answers() {
    platform_api::infrastructure::load_environment().unwrap();
    let dir = std::env::temp_dir().join(format!("proctor-evidence-test-{}", Uuid::new_v4()));
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
    let stranger = account(&app).await;
    let (invite, _) = setup(&pool, &app, &owner).await;
    let (other, _) = setup(&pool, &app, &owner).await;
    let candidate = invite["invitation_token"].as_str().unwrap();
    let session: Uuid = invite["session"]["id"].as_str().unwrap().parse().unwrap();
    let owner_id = owner["user_id"].as_str().unwrap().parse().unwrap();
    let mut tx = interviewer_transaction(&pool, owner_id).await.unwrap();
    sqlx::query("UPDATE interview_sessions SET state='in_progress',started_at=now(),deadline_at=now()+interval '30 minutes' WHERE id=$1").bind(session).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let event = Uuid::new_v4();
    let asset = Uuid::new_v4();
    let next_event = Uuid::new_v4();
    let observation = json!([
        {"id":event,"kind":"multiple_faces","client_time":1,"measurements":{"duration_ms":3000,"face_count":2}},
        {"id":next_event,"kind":"face_missing","client_time":2,"measurements":{"duration_ms":5000,"face_count":0}}
    ]);
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/candidate/events",
            observation.clone(),
            candidate
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/candidate/events",
            observation,
            candidate
        )
        .await
        .0,
        StatusCode::OK
    );
    let upload = format!("/api/v1/candidate/events/{event}/evidence/{asset}");
    let mut jpeg = vec![0u8; 64];
    jpeg[..3].copy_from_slice(&[0xff, 0xd8, 0xff]);
    jpeg[62..].copy_from_slice(&[0xff, 0xd9]);
    assert_eq!(
        binary(&app, "POST", &upload, "image/jpeg", jpeg.clone(), candidate)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        binary(&app, "POST", &upload, "image/jpeg", jpeg.clone(), candidate)
            .await
            .0,
        StatusCode::OK
    );
    let mut changed = jpeg.clone();
    changed[20] = 1;
    assert_eq!(
        binary(&app, "POST", &upload, "image/jpeg", changed, candidate)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        binary(
            &app,
            "POST",
            &format!(
                "/api/v1/candidate/events/{event}/evidence/{}",
                Uuid::new_v4()
            ),
            "image/jpeg",
            jpeg.clone(),
            candidate
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        binary(
            &app,
            "POST",
            &format!(
                "/api/v1/candidate/events/{}/evidence/{}",
                Uuid::new_v4(),
                Uuid::new_v4()
            ),
            "image/jpeg",
            jpeg.clone(),
            candidate
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        binary(
            &app,
            "POST",
            &upload,
            "image/jpeg",
            jpeg.clone(),
            other["invitation_token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let read = format!("/api/v1/sessions/{session}/evidence/{asset}");
    let (status, body) = binary(
        &app,
        "GET",
        &read,
        "",
        vec![],
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, jpeg);
    assert_eq!(
        binary(
            &app,
            "GET",
            &read,
            "",
            vec![],
            stranger["token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        binary(&app, "GET", &read, "", vec![], candidate).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, report) = call(
        &app,
        "GET",
        &format!("/api/v1/sessions/{session}/integrity"),
        Value::Null,
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        report["events"][0]["attachments"][0]["asset_id"],
        json!(asset)
    );
    assert!(
        report["events"]
            .as_array()
            .unwrap()
            .iter()
            .all(|event| event["chain_status"] == "valid_server_receipt")
    );
    let (_, answers) = call(
        &app,
        "GET",
        "/api/v1/candidate/recordings",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(answers["recordings"], json!([]));
    assert_eq!(
        binary(
            &app,
            "POST",
            &format!(
                "/api/v1/candidate/events/{event}/evidence/{}",
                Uuid::new_v4()
            ),
            "audio/webm",
            vec![0; 2 * 1024 * 1024 + 1],
            candidate
        )
        .await
        .0,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let file = dir
        .join(owner_id.to_string())
        .join(session.to_string())
        .join(format!("{asset}.evidence"));
    tokio::fs::write(file, b"tampered bytes").await.unwrap();
    assert_eq!(
        binary(
            &app,
            "GET",
            &read,
            "",
            vec![],
            owner["token"].as_str().unwrap()
        )
        .await
        .0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    pool.close().await;
}
