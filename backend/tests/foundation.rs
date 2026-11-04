use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use platform_api::infrastructure::tenant_transaction;
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
    let app = platform_api::router(pool.clone());
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
    assert_eq!(a["role"], "recruiter");
    let token = a["token"].as_str().unwrap();
    let org: Uuid = a["organization_id"].as_str().unwrap().parse().unwrap();
    let (status, _) = call(&app, "POST", "/api/auth/register", credentials, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, b) = call(&app,"POST","/api/auth/register",json!({"email":format!("{}@example.test",Uuid::new_v4()),"password":"another-strong-password"}),None).await;
    assert_eq!(status, StatusCode::CREATED);
    let other = b["organization_id"].as_str().unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/organizations/{org}"),
            Value::Null,
            Some(token)
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/organizations/{other}"),
            Value::Null,
            Some(token)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
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
    assert_eq!(signed_in["organization_id"], a["organization_id"]);
    assert_eq!(signed_in["role"], "recruiter");
    let (_, profile) = call(&app, "GET", "/api/me", Value::Null, Some(token)).await;
    assert_eq!(profile["organization_id"], a["organization_id"]);
    assert_eq!(profile["role"], "recruiter");
    // Raw SQL with no tenant cannot read either organization.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM organizations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let mut tx = tenant_transaction(&pool, org).await.unwrap();
    let ids: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM organizations")
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    assert_eq!(ids, vec![org]);
    tx.commit().await.unwrap();
    // Transaction-local context must not leak through the connection pool.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM organizations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let mut tx = tenant_transaction(&pool, org).await.unwrap();
    let denied = sqlx::query("INSERT INTO organizations(id,name) VALUES ($1,'Forbidden')")
        .bind(Uuid::new_v4())
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
