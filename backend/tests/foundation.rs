use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use platform_api::infrastructure::interviewer_transaction;
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use tower::ServiceExt;
use uuid::Uuid;

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
#[ignore = "requires migrated PostgreSQL, runtime DATABASE_URL, and ALLOW_REGISTRATION=true"]
async fn authentication_and_tenant_isolation() {
    platform_api::infrastructure::load_environment().unwrap();
    assert_eq!(std::env::var("ALLOW_REGISTRATION").as_deref(), Ok("true"));
    let pool = PgPoolOptions::new()
        .max_connections(2)
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
    assert!(!bypass, "tests must use the restricted runtime role");
    let app = platform_api::router_with_ai(pool.clone(), None);
    let email = format!("{}@example.test", Uuid::new_v4());
    let credentials = json!({"email":email,"password":"a-strong-test-password"});
    let (status, a) = call(
        &app,
        "POST",
        "/api/auth/register",
        credentials.clone(),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(a["role"], "interviewer");
    let token = a["token"].as_str().unwrap();
    let recruiter: Uuid = a["user_id"].as_str().unwrap().parse().unwrap();
    let (status, _) = call(&app, "POST", "/api/auth/register", credentials, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, b) = call(&app,"POST","/api/auth/register",json!({"email":format!("{}@example.test",Uuid::new_v4()),"password":"another-strong-password"}),None).await;
    assert_eq!(status, StatusCode::CREATED);
    let other: Uuid = b["user_id"].as_str().unwrap().parse().unwrap();
    let job_input = json!({"title":"Backend engineer","description":"Build reliable Rust services and PostgreSQL APIs.","request_key":Uuid::new_v4()});
    let (status, job) = call(&app, "POST", "/api/v1/jobs", job_input, Some(token)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (_, mine) = call(&app, "GET", "/api/v1/jobs", Value::Null, Some(token)).await;
    assert_eq!(mine["items"][0]["id"], job["id"]);
    let (_, theirs) = call(
        &app,
        "GET",
        "/api/v1/jobs",
        Value::Null,
        b["token"].as_str(),
    )
    .await;
    assert!(theirs["items"].as_array().unwrap().is_empty());
    assert!(a.get("organization_id").is_none());
    assert_eq!(
        call(&app, "GET", "/api/me", Value::Null, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/login",
            json!({"email":email,"password":"incorrect-password"}),
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/auth/login",
            json!({"email":email,"password":"a-strong-test-password"}),
            None
        )
        .await
        .0,
        StatusCode::OK
    );

    let (_, signed_in) = call(
        &app,
        "POST",
        "/api/auth/login",
        json!({"email":email,"password":"a-strong-test-password"}),
        None,
    )
    .await;
    assert_eq!(signed_in["user_id"], a["user_id"]);
    assert_eq!(signed_in["role"], "interviewer");
    let (_, profile) = call(&app, "GET", "/api/me", Value::Null, Some(token)).await;
    assert_eq!(profile["user_id"], a["user_id"]);
    assert_eq!(profile["role"], "interviewer");
    // Raw SQL without authenticated recruiter context cannot read owned records.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT interviewer_id FROM jobs")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(ids, vec![recruiter]);
    tx.commit().await.unwrap();
    // Transaction-local context must not leak through the connection pool.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let mut tx = interviewer_transaction(&pool, recruiter).await.unwrap();
    let denied = sqlx::query("INSERT INTO jobs(id,interviewer_id,request_key,title,created_by) VALUES($1,$2,$3,'Forbidden',$2)")
        .bind(Uuid::new_v4()).bind(other).bind(Uuid::new_v4())
        .execute(&mut *tx)
        .await;
    assert!(denied.is_err());
    tx.rollback().await.unwrap();
    assert_eq!(
        call(&app, "POST", "/api/auth/logout", Value::Null, Some(token))
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(&app, "GET", "/api/me", Value::Null, Some(token))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    pool.close().await;
}
