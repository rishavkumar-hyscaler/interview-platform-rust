mod common;
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
    let(s,job)=call(app,"POST","/api/v1/jobs",json!({"job_code":format!("TEST-{}",Uuid::new_v4()),"title":"Interview room test","description":"Build reliable applications in Rust and PostgreSQL.","preparation_duration_minutes":30,"request_key":Uuid::new_v4()}),token).await;
    assert_eq!(s, StatusCode::CREATED);
    let job: Uuid = job["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    common::seed_set(pool, user, job, 30).await;
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
    let(s,mut invite)=call(app,"POST",&format!("/api/v1/jobs/{job}/invitations"),json!({"candidate_name":"Room test candidate","candidate_email":"room-test@example.test","duration_minutes":30,"request_key":Uuid::new_v4()}),token).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    common::publish_fixture(pool, user, &mut invite).await;
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
    binary_with_headers(app, method, path, mime, bytes, token, &[]).await
}
async fn binary_with_headers(
    app: &Router,
    method: &str,
    path: &str,
    mime: &str,
    bytes: Vec<u8>,
    token: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", mime);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(bytes)).unwrap())
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
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let app = platform_api::router_with_ai(pool.clone(), None);
    let owner = account(&app).await;
    let (invite, questions) = setup(&pool, &app, &owner).await;
    let (other, other_questions) = setup(&pool, &app, &owner).await;
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
    tokio::fs::write(&orphan, b"partial upload")
        .await
        .unwrap();
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
            other["invitation_token"]
                .as_str()
                .unwrap()
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
    tokio::fs::write(&file, b"modified")
        .await
        .unwrap();
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
    tokio::fs::write(&file, &bytes)
        .await
        .unwrap();
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
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
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
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let app = platform_api::router_with_ai(pool.clone(), None);
    let owner = account(&app).await;
    let stranger = account(&app).await;
    let (invite, _) = setup(&pool, &app, &owner).await;
    let (other, _) = setup(&pool, &app, &owner).await;
    let candidate = invite["invitation_token"]
        .as_str()
        .unwrap();
    let session: Uuid = invite["session"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let owner_id = owner["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    sqlx::query("UPDATE interview_sessions SET state='in_progress',started_at=now(),deadline_at=now()+interval '30 minutes' WHERE id=$1").bind(session).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let event = Uuid::new_v4();
    let asset = Uuid::new_v4();
    let next_event = Uuid::new_v4();
    let observation = json!([
        {"id":event,"kind":"multiple_faces","client_time":1,"measurements":{"duration_ms":3000,"face_count":2}},
        {"id":next_event,"kind":"face_missing","client_time":2,"measurements":{"duration_ms":5197,"face_count":0}}
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
            other["invitation_token"]
                .as_str()
                .unwrap()
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
        binary(&app, "GET", &read, "", vec![], candidate)
            .await
            .0,
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
    // Both video and the pre-signal speech WAV survive storage and private reads.
    let mut video = vec![0u8; 64];
    video[..4].copy_from_slice(&[0x1a, 0x45, 0xdf, 0xa3]);
    let mut wav = vec![0u8; 44 + 16000 * 2 * 2];
    wav[..4].copy_from_slice(b"RIFF");
    let length = wav.len();
    wav[4..8].copy_from_slice(&((length - 8) as u32).to_le_bytes());
    wav[8..16].copy_from_slice(b"WAVEfmt ");
    wav[16..24].copy_from_slice(&[16, 0, 0, 0, 1, 0, 1, 0]);
    wav[24..28].copy_from_slice(&16000u32.to_le_bytes());
    wav[28..32].copy_from_slice(&32000u32.to_le_bytes());
    wav[32..40].copy_from_slice(&[2, 0, 16, 0, b'd', b'a', b't', b'a']);
    wav[40..44].copy_from_slice(&((length - 44) as u32).to_le_bytes());
    for (kind, mime, bytes, capture) in [
        ("face_missing", "video/webm", video, "post_signal"),
        (
            "possible_background_speech",
            "audio/wav",
            wav,
            "signal_window",
        ),
    ] {
        let observed = Uuid::new_v4();
        let attachment = Uuid::new_v4();
        assert_eq!(call(&app,"POST","/api/v1/candidate/events",json!([{"id":observed,"kind":kind,"client_time":3,"measurements":{"duration_ms":2000}}]),candidate).await.0,StatusCode::OK);
        let upload = format!("/api/v1/candidate/events/{observed}/evidence/{attachment}");
        assert_eq!(
            binary_with_headers(
                &app,
                "POST",
                &upload,
                mime,
                bytes.clone(),
                candidate,
                &[("x-evidence-capture", capture)]
            )
            .await
            .0,
            StatusCode::OK
        );
        let read = format!("/api/v1/sessions/{session}/evidence/{attachment}");
        let (status, served) = binary(
            &app,
            "GET",
            &read,
            "",
            vec![],
            owner["token"].as_str().unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(served, bytes);
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
    }
    let file = dir
        .join(owner_id.to_string())
        .join(session.to_string())
        .join(format!("{asset}.evidence"));
    tokio::fs::write(file, b"tampered bytes")
        .await
        .unwrap();
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

#[tokio::test]
#[ignore = "requires migrated PostgreSQL and private temporary evidence storage"]
async fn permission_denials_are_logged_before_start_and_screen_evidence_is_private() {
    platform_api::infrastructure::load_environment().unwrap();
    let dir = std::env::temp_dir().join(format!("screen-evidence-test-{}", Uuid::new_v4()));
    let _cleanup = Cleanup(dir.clone());
    unsafe {
        std::env::set_var("MEDIA_STORAGE_DIR", &dir);
    }
    let pool = PgPoolOptions::new()
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap();
    let app = platform_api::router_with_ai(pool.clone(), None);
    let owner = account(&app).await;
    let stranger = account(&app).await;
    let (invite, _) = setup(&pool, &app, &owner).await;
    let token = invite["invitation_token"]
        .as_str()
        .unwrap();
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
    let denied = Uuid::new_v4();
    let sharing = Uuid::new_v4();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/candidate/events",
            json!([
                {"id":denied,"kind":"camera_denied","client_time":1},
                {"id":Uuid::new_v4(),"kind":"microphone_denied","client_time":2},
                {"id":sharing,"kind":"screen_share_started","client_time":3}
            ]),
            token
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
            json!([{"id":Uuid::new_v4(),"kind":"paste","client_time":4}]),
            token
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let mut tx = interviewer_transaction(&pool, owner_id)
        .await
        .unwrap();
    let stage: String =
        sqlx::query_scalar("SELECT metadata->>'stage' FROM audit_events WHERE id=$1")
            .bind(denied)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(stage, "preflight");
    tx.commit().await.unwrap();
    let mut jpeg = vec![0u8; 64];
    jpeg[..3].copy_from_slice(&[0xff, 0xd8, 0xff]);
    jpeg[62..].copy_from_slice(&[0xff, 0xd9]);
    let headers = [
        ("x-evidence-source", "screen"),
        ("x-capture-surface", "monitor"),
        ("x-evidence-captured-at", "1234"),
    ];
    assert_eq!(
        binary_with_headers(
            &app,
            "POST",
            &format!(
                "/api/v1/candidate/events/{sharing}/evidence/{}",
                Uuid::new_v4()
            ),
            "image/jpeg",
            jpeg.clone(),
            token,
            &headers
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/candidate/session/ready",
            json!({}),
            token
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
            token
        )
        .await
        .0,
        StatusCode::OK
    );
    let event = Uuid::new_v4();
    let asset = Uuid::new_v4();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/candidate/events",
            json!([{"id":event,"kind":"fullscreen_entered","client_time":1000}]),
            token
        )
        .await
        .0,
        StatusCode::OK
    );
    let path = format!("/api/v1/candidate/events/{event}/evidence/{asset}");
    let (status, body) = binary_with_headers(
        &app,
        "POST",
        &path,
        "image/jpeg",
        jpeg.clone(),
        token,
        &headers,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let metadata: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(metadata["evidence_source"], "screen");
    assert_eq!(metadata["capture_surface"], "monitor");
    assert_eq!(metadata["captured_at_ms"], 1234);
    assert_eq!(metadata["verified"], false);
    assert_eq!(
        binary_with_headers(
            &app,
            "POST",
            &path,
            "image/jpeg",
            jpeg.clone(),
            token,
            &headers
        )
        .await
        .0,
        StatusCode::OK
    );
    let file = dir
        .join(owner_id.to_string())
        .join(session.to_string())
        .join("screenshots")
        .join(format!("{asset}.jpg"));
    assert!(file.exists());
    let read = format!("/api/v1/sessions/{session}/evidence/{asset}");
    let (status, bytes) = binary(
        &app,
        "GET",
        &read,
        "",
        vec![],
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, jpeg);
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
        binary(&app, "GET", &read, "", vec![], token)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (_, answers) = call(
        &app,
        "GET",
        "/api/v1/candidate/recordings",
        Value::Null,
        token,
    )
    .await;
    assert_eq!(answers["recordings"], json!([]));
    let (_, report) = call(
        &app,
        "GET",
        &format!("/api/v1/sessions/{session}/integrity"),
        Value::Null,
        owner["token"].as_str().unwrap(),
    )
    .await;
    assert!(
        report["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| {
                e["attachments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["asset_id"] == asset.to_string() && a["evidence_source"] == "screen")
            })
    );
    pool.close().await;
}
