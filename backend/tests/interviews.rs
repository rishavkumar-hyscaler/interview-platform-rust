mod common;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use platform_api::infrastructure::{interviewer_transaction, process_expiry_job};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

async fn call(
    app: &axum::Router,
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
async fn account(app: &axum::Router) -> Value {
    let (status,result)=call(app,"POST","/api/auth/register",json!({"email":format!("{}@example.test",Uuid::new_v4()),"password":"phase-one-test-password"}),"").await;
    assert_eq!(status, StatusCode::CREATED);
    result
}
async fn database() -> PgPool {
    platform_api::infrastructure::load_environment().unwrap();
    assert_eq!(std::env::var("ALLOW_REGISTRATION").as_deref(), Ok("true"));
    let pool = PgPoolOptions::new()
        .max_connections(5)
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
    pool
}
async fn ready_set(pool: &PgPool, owner: Uuid, job: Uuid, duration: i32) {
    common::seed_set(pool, owner, job, duration).await;
    let mut tx = interviewer_transaction(pool, owner)
        .await
        .unwrap();
    let set: Uuid = sqlx::query_scalar(
        "SELECT id FROM question_sets WHERE job_id=$1 ORDER BY created_at,id LIMIT 1",
    )
    .bind(job)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let question = Uuid::new_v4();
    sqlx::query("INSERT INTO interview_questions(id,interviewer_id,set_id,job_id,fingerprint,position,kind,difficulty,prompt,rubric) VALUES($1,$2,$3,$4,$5,1,'text','medium','Explain transaction isolation with an example','{\"criteria\":[]}'::jsonb)")
        .bind(question).bind(owner).bind(set).bind(job).bind(question.to_string()).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE question_sets SET status='completed',duration_minutes=$2 WHERE id=$1")
        .bind(set)
        .bind(duration)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
#[ignore = "requires migrated PostgreSQL and restricted DATABASE_URL; ALLOW_REGISTRATION=true"]
async fn interview_lifecycle_is_scoped_idempotent_and_audited() {
    let pool = database().await;
    let app = platform_api::router_with_ai(pool.clone(), None);
    let recruiter_account = account(&app).await;
    let outsider = account(&app).await;
    let recruiter: Uuid = recruiter_account["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let token = recruiter_account["token"]
        .as_str()
        .unwrap();
    let other = outsider["token"].as_str().unwrap();
    assert_eq!(recruiter_account["role"], "interviewer");
    let base = "/api/v1".to_string();
    let job_input = json!({"job_code":"  be-2026-001  ","title":"Backend engineer","preparation_duration_minutes":30,"description":"Build reliable Rust APIs and PostgreSQL services.","request_key":Uuid::new_v4()});
    let (status, job) = call(
        &app,
        "POST",
        &format!("{base}/jobs"),
        job_input.clone(),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, retry) = call(
        &app,
        "POST",
        &format!("{base}/jobs"),
        job_input.clone(),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(job["id"], retry["id"]);
    assert_eq!(job["job_code"], "BE-2026-001");
    let mut duplicate = job_input.clone();
    duplicate["request_key"] = json!(Uuid::new_v4());
    duplicate["job_code"] = json!("Be-2026-001");
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/jobs"),
            duplicate.clone(),
            token
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(&app, "POST", &format!("{base}/jobs"), duplicate, other)
            .await
            .0,
        StatusCode::CREATED
    );
    let mut invalid = job_input.clone();
    invalid["request_key"] = json!(Uuid::new_v4());
    invalid["job_code"] = json!("../invalid job");
    assert_eq!(
        call(&app, "POST", &format!("{base}/jobs"), invalid, token)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut missing = job_input.clone();
    missing
        .as_object_mut()
        .unwrap()
        .remove("job_code");
    assert_eq!(
        call(&app, "POST", &format!("{base}/jobs"), missing, token)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let mut changed = job_input.clone();
    changed["title"] = json!("Different job");
    assert_eq!(
        call(&app, "POST", &format!("{base}/jobs"), changed, token)
            .await
            .0,
        StatusCode::CONFLICT
    );
    // Another job exercises cursor ordering and pagination without offset drift.
    let (_,second)=call(&app,"POST",&format!("{base}/jobs"),json!({"job_code":format!("TEST-{}",Uuid::new_v4()),"title":"Frontend engineer","preparation_duration_minutes":30,"description":"Build accessible React interfaces and browser integrations.","request_key":Uuid::new_v4()}),token).await;
    let (_, page) = call(
        &app,
        "GET",
        &format!("{base}/jobs?limit=1"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["id"], second["id"]);
    let (_, next) = call(
        &app,
        "GET",
        &format!(
            "{base}/jobs?limit=1&cursor={}",
            page["next_cursor"].as_str().unwrap()
        ),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(next["items"][0]["id"], job["id"]);
    assert!(next["next_cursor"].is_null());
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("{base}/jobs?limit=0"),
            Value::Null,
            token
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    ready_set(
        &pool,
        recruiter,
        job["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        30,
    )
    .await;
    let invitation = json!({"candidate_name":"Candidate A","candidate_email":"candidate@example.test","duration_minutes":30,"request_key":Uuid::new_v4()});
    let path = format!("{base}/jobs/{}/invitations", job["id"].as_str().unwrap());
    let (status, mut invite) = call(&app, "POST", &path, invitation.clone(), token).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    common::publish_fixture(&pool, recruiter, &mut invite).await;
    let id = invite["session"]["id"]
        .as_str()
        .unwrap();
    let candidate = invite["invitation_token"]
        .as_str()
        .unwrap();
    let (status, repeated) = call(&app, "POST", &path, invitation, token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repeated["session"]["id"], id);
    assert_eq!(repeated["invitation_token"], candidate);
    let (status, recovered) = call(
        &app,
        "POST",
        &format!("{base}/sessions/{id}/invitation"),
        json!({}),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recovered["invitation_token"], candidate);
    assert_eq!(recovered["replaced_legacy_link"], false);
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/sessions/{id}/invitation"),
            json!({}),
            other
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/sessions/{id}/invitation"),
            json!({}),
            candidate
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let sealed: String =
        sqlx::query_scalar("SELECT sealed_token FROM auth_sessions WHERE interview_session_id=$1")
            .bind(id.parse::<Uuid>().unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!sealed.contains(candidate));
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("{base}/sessions/{id}"),
            Value::Null,
            other
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("{base}/sessions/{id}"),
            Value::Null,
            candidate
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (_, view) = call(
        &app,
        "GET",
        "/api/v1/candidate/session",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(view["id"], id);
    assert_eq!(view["role"], "interviewee");
    let (candidate_login, _) = call(
        &app,
        "POST",
        "/api/auth/login",
        json!({"email":"candidate@example.test","password":"not-an-interviewer-password"}),
        "",
    )
    .await;
    assert_eq!(candidate_login, StatusCode::UNAUTHORIZED);
    assert!(view.get("candidate_email").is_none());
    assert!(view.get("rubric").is_none());
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/candidate/session/submit",
            Value::Null,
            candidate
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    for _ in 0..2 {
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/v1/candidate/session/ready",
                Value::Null,
                candidate
            )
            .await
            .1["state"],
            "ready"
        );
    }
    let (_, events) = call(
        &app,
        "GET",
        &format!("{base}/audit?entity_id={id}"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(
        events["items"]
            .as_array()
            .unwrap()
            .len(),
        4,
        "set assignment, invitation, link retrieval, and acceptance are audited"
    );
    let session: Uuid = id.parse().unwrap();
    let (_, started) = call(
        &app,
        "POST",
        "/api/v1/candidate/session/start",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(started["state"], "in_progress");
    let (_, start_retry) = call(
        &app,
        "POST",
        "/api/v1/candidate/session/start",
        Value::Null,
        candidate,
    )
    .await;
    assert_eq!(started["deadline_at"], start_retry["deadline_at"]);
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
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/candidate/session/start",
            Value::Null,
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
            &format!("{base}/sessions/{id}/invitation"),
            json!({}),
            token
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    // An expiry job after submission must not overwrite submitted state.
    let _ = process_expiry_job(&pool).await.unwrap();
    let (_, detail) = call(
        &app,
        "GET",
        &format!("{base}/sessions/{id}"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(detail["state"], "submitted");
    assert!(detail.get("jd_version").is_none());
    let (_, counts) = call(&app, "GET", &format!("{base}/summary"), Value::Null, token).await;
    assert_eq!(counts["jobs"], 2);
    assert_eq!(counts["sessions"], 1);
    assert_eq!(counts["reviewable"], 1);
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM interview_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(visible, 0);
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE audit_events SET action='forged' WHERE entity_id=$1")
            .bind(session)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires migrated PostgreSQL and restricted DATABASE_URL; ALLOW_REGISTRATION=true"]
async fn concurrent_workers_expire_once_and_tenant_foreign_keys_hold() {
    let pool = database().await;
    let app = platform_api::router_with_ai(pool.clone(), None);
    let recruiter_account = account(&app).await;
    let other = account(&app).await;
    let recruiter: Uuid = recruiter_account["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let other_org: Uuid = other["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let token = recruiter_account["token"]
        .as_str()
        .unwrap();
    let base = "/api/v1".to_string();
    let (_,job)=call(&app,"POST",&format!("{base}/jobs"),json!({"job_code":format!("TEST-{}",Uuid::new_v4()),"title":"Expiry test","preparation_duration_minutes":5,"description":"A role for testing durable expiry and tenant boundaries.","request_key":Uuid::new_v4()}),token).await;
    ready_set(
        &pool,
        recruiter,
        job["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        5,
    )
    .await;
    let (_,mut invite)=call(&app,"POST",&format!("{base}/jobs/{}/invitations",job["id"].as_str().unwrap()),json!({"candidate_name":"Expiry candidate","candidate_email":"expiry@example.test","duration_minutes":5,"request_key":Uuid::new_v4()}),token).await;
    common::publish_fixture(&pool, recruiter, &mut invite).await;
    let id: Uuid = invite["session"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let candidate = invite["invitation_token"]
        .as_str()
        .unwrap();
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    sqlx::query("UPDATE interview_sessions SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/candidate/session/ready",
            Value::Null,
            candidate
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (shutdown, receiver) = tokio::sync::watch::channel(false);
    let a = tokio::spawn(platform_api::worker::run(pool.clone(), receiver.clone()));
    let b = tokio::spawn(platform_api::worker::run(pool.clone(), receiver));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let mut tx = interviewer_transaction(&pool, recruiter)
                .await
                .unwrap();
            let state: String =
                sqlx::query_scalar("SELECT state FROM interview_sessions WHERE id=$1")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
            tx.commit().await.unwrap();
            if state == "expired" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    shutdown.send(true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        a.await.unwrap();
        b.await.unwrap();
    })
    .await
    .unwrap();
    let (_, detail) = call(
        &app,
        "GET",
        &format!("{base}/sessions/{id}"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(detail["state"], "expired");
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE entity_id=$1 AND action='session.expired'",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(count, 1);
    tx.commit().await.unwrap();
    let mut tx = interviewer_transaction(&pool, other_org)
        .await
        .unwrap();
    let foreign_job: Uuid = job["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let result=sqlx::query("INSERT INTO interview_sessions(id,interviewer_id,interviewee_id,job_id,request_key,candidate_name,candidate_email,duration_minutes) VALUES($1,$2,$3,$4,$5,'Forbidden','forbidden@example.test',30)")
        .bind(Uuid::new_v4()).bind(other_org).bind(recruiter).bind(foreign_job).bind(Uuid::new_v4()).execute(&mut *tx).await;
    assert!(result.is_err());
    tx.rollback().await.unwrap();
    pool.close().await;
}

#[tokio::test]
async fn invalid_requests_have_json_error_contracts_without_database_access() {
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused@localhost/unused")
        .unwrap();
    let app = platform_api::router_with_ai(pool, None);
    let (status, error) = call(&app, "POST", "/api/auth/register", json!({
        "email":"unsupported-role@example.test", "password":"valid-test-password", "role":"owner"
    }), "").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["code"], "invalid_request");
    let base = "/api/v1".to_string();
    let (status, error) = call(
        &app,
        "POST",
        &format!("{base}/jobs"),
        json!({"job_code":format!("TEST-{}",Uuid::new_v4()),"title":"missing required fields"}),
        "",
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["code"], "invalid_request");
    let (status, error) = call(
        &app,
        "GET",
        &format!("{base}/sessions?cursor=not-a-uuid"),
        Value::Null,
        "",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["code"], "invalid_request");
    let (status, error) = call(&app, "GET", "/missing-route", Value::Null, "").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["code"], "invalid_request");
    let (status, error) = call(&app, "GET", "/api/v1/candidate/session", Value::Null, "").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["code"], "unauthorized");
}

#[tokio::test]
#[ignore = "requires migrated PostgreSQL and restricted DATABASE_URL; ALLOW_REGISTRATION=true"]
async fn legacy_invitation_replacement_is_explicit_scoped_and_preserves_expiry() {
    let pool = database().await;
    let app = platform_api::router_with_ai(pool.clone(), None);
    let recruiter_account = account(&app).await;
    let recruiter: Uuid = recruiter_account["user_id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let token = recruiter_account["token"]
        .as_str()
        .unwrap();
    let base = "/api/v1".to_string();
    let (_,job)=call(&app,"POST",&format!("{base}/jobs"),json!({"job_code":format!("TEST-{}",Uuid::new_v4()),"title":"Legacy link test","preparation_duration_minutes":30,"description":"A job description used for testing safe invitation recovery.","request_key":Uuid::new_v4()}),token).await;
    ready_set(
        &pool,
        recruiter,
        job["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap(),
        30,
    )
    .await;
    let (_,mut invite)=call(&app,"POST",&format!("{base}/jobs/{}/invitations",job["id"].as_str().unwrap()),json!({"candidate_name":"Legacy candidate","candidate_email":"legacy@example.test","duration_minutes":30,"request_key":Uuid::new_v4()}),token).await;
    common::publish_fixture(&pool, recruiter, &mut invite).await;
    let id: Uuid = invite["session"]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let original = invite["invitation_token"]
        .as_str()
        .unwrap();
    // Simulate a pre-upgrade invitation, for which the original secret was not retained.
    sqlx::query("UPDATE auth_sessions SET sealed_token=NULL WHERE interview_session_id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let (_, detail) = call(
        &app,
        "GET",
        &format!("{base}/sessions/{id}"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(detail["invitation_recoverable"], false);
    assert_eq!(detail["invitation_active"], true);
    let (status, reissued) = call(
        &app,
        "POST",
        &format!("{base}/sessions/{id}/invitation"),
        json!({}),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reissued["replaced_legacy_link"], true);
    let replacement = reissued["invitation_token"]
        .as_str()
        .unwrap();
    assert_ne!(original, replacement);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/candidate/session",
            Value::Null,
            original
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/candidate/session",
            Value::Null,
            replacement
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, again) = call(
        &app,
        "POST",
        &format!("{base}/sessions/{id}/invitation"),
        json!({}),
        token,
    )
    .await;
    assert_eq!(again["invitation_token"], replacement);
    assert_eq!(again["replaced_legacy_link"], false);
    let (_, after) = call(
        &app,
        "GET",
        &format!("{base}/sessions/{id}"),
        Value::Null,
        token,
    )
    .await;
    assert_eq!(after["expires_at"], invite["session"]["expires_at"]);
    let (_, events) = call(
        &app,
        "GET",
        &format!("{base}/audit?entity_id={id}"),
        Value::Null,
        token,
    )
    .await;
    assert!(!events.to_string().contains(original));
    assert!(!events.to_string().contains(replacement));
    assert!(
        events["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "invitation.link_reissued")
    );
    call(
        &app,
        "POST",
        "/api/v1/candidate/session/ready",
        Value::Null,
        replacement,
    )
    .await;
    sqlx::query("UPDATE auth_sessions SET sealed_token=NULL WHERE interview_session_id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/sessions/{id}/invitation"),
            json!({}),
            token
        )
        .await
        .0,
        StatusCode::CONFLICT,
        "do not invalidate a legacy candidate who already accepted"
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/candidate/session",
            Value::Null,
            replacement
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut tx = interviewer_transaction(&pool, recruiter)
        .await
        .unwrap();
    sqlx::query("UPDATE interview_sessions SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("{base}/sessions/{id}/invitation"),
            json!({}),
            token
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    pool.close().await;
}

#[tokio::test]
#[ignore = "requires migrated PostgreSQL and restricted runtime credentials"]
async fn concurrent_job_ids_are_unique_and_case_insensitive() {
    let pool = database().await;
    let app = platform_api::router_with_ai(pool.clone(), None);
    let owner = account(&app).await;
    let token = owner["token"].as_str().unwrap();
    let input = |code: &str| json!({"job_code":code,"title":"Concurrent code test","preparation_duration_minutes":30,"description":"A sufficiently detailed job description for concurrency tests.","request_key":Uuid::new_v4()});
    let (a, b) = tokio::join!(
        call(&app, "POST", "/api/v1/jobs", input("race-001"), token),
        call(&app, "POST", "/api/v1/jobs", input("RACE-001"), token)
    );
    assert!(
        (a.0 == StatusCode::CREATED && b.0 == StatusCode::CONFLICT)
            || (b.0 == StatusCode::CREATED && a.0 == StatusCode::CONFLICT)
    );
    let (_, jobs) = call(&app, "GET", "/api/v1/jobs", Value::Null, token).await;
    assert_eq!(jobs["items"].as_array().unwrap().len(), 1);
    assert_eq!(jobs["items"][0]["job_code"], "RACE-001");
}
