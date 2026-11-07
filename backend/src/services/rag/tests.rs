//! One isolated database test with a local embedding mock. Never calls a live provider.
use super::*;
use crate::{generation::provider::AiProvider, infrastructure::interviewer_transaction};
use axum::{Json, Router, extract::State, response::IntoResponse};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;

#[derive(Clone)]
struct Mock {
    batches: Arc<Mutex<Vec<usize>>>,
    fail_at: Arc<AtomicUsize>,
}
async fn embeddings(
    State(mock): State<Mock>,
    Json(input): Json<Value>,
) -> axum::response::Response {
    let count = input["input"].as_array().unwrap().len();
    let call = {
        let mut batches = mock.batches.lock().unwrap();
        batches.push(count);
        batches.len()
    };
    if mock.fail_at.load(Ordering::SeqCst) == call {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error":{"code":"rate_limit_exceeded"}})),
        )
            .into_response();
    }
    Json(json!({"object":"list","model":"text-embedding-3-small","usage":{"prompt_tokens":1,"total_tokens":1},"data":(0..count).map(|index|json!({"object":"embedding","index":index,"embedding":[1.0,0.5,0.25]})).collect::<Vec<_>>()})).into_response()
}
async fn owner(pool: &PgPool) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO users(id,
                email,
                password_hash)
        VALUES ($1,
                $2,
                'test-only')
        "#,
    )
    .bind(id)
    .bind(format!("{id}@example.test"))
    .execute(pool)
    .await
    .unwrap();
    id
}
async fn setup(pool: &PgPool, owner: Uuid) -> (Uuid, Uuid) {
    let job = Uuid::new_v4();
    let set = Uuid::new_v4();
    let question = Uuid::new_v4();
    let candidate = Uuid::new_v4();
    let session = Uuid::new_v4();
    let mut tx = interviewer_transaction(pool, owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users(id,email,role) VALUES($1,$2,'interviewee')")
        .bind(candidate)
        .bind(format!("{candidate}@example.test"))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        r#"
        INSERT INTO jobs(id,
                interviewer_id,
                request_key,
                job_code,
                title,
                description)
        VALUES ($1,
                $2,
                $3,
                $4,
                'RAG test',
                'Build reliable Rust services and APIs')
        "#,
    )
    .bind(job)
    .bind(owner)
    .bind(Uuid::new_v4())
    .bind(format!("TEST-{job}").to_uppercase())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO question_sets(id,
                interviewer_id,
                job_id,
                duration_minutes,
                status,
                seed)
        VALUES ($1,
                $2,
                $3,
                30,
                'completed',
                $4)
        "#,
    )
    .bind(set)
    .bind(owner)
    .bind(job)
    .bind(Uuid::new_v4())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO interview_questions(id,
                interviewer_id,
                set_id,
                position,
                kind,
                difficulty,
                prompt,
                rubric,
                job_id,
                fingerprint)
        VALUES ($1,
                $2,
                $3,
                1,
                'text',
                'easy',
                'Explain Rust ownership',
                '{"expected_answer":"Ownership protects memory"}',
                $4,
                $5)
        "#,
    )
    .bind(question)
    .bind(owner)
    .bind(set)
    .bind(job)
    .bind(question.to_string())
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO interview_sessions(id,
                interviewer_id,
                job_id,
                request_key,
                question_set_id,
                candidate_name,
                candidate_email,
                state,
                duration_minutes,
                interviewee_id)
        VALUES ($1,
                $2,
                $3,
                $4,
                $5,
                'Test candidate',
                'candidate@example.test',
                'completed',
                30,
                $6)
        "#,
    )
    .bind(session)
    .bind(owner)
    .bind(job)
    .bind(Uuid::new_v4())
    .bind(set)
    .bind(candidate)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO interview_answers(id,
                interviewer_id,
                session_id,
                question_id,
                content)
        VALUES ($1,
                $2,
                $3,
                $4,
                'The answer uses borrowing for safe memory')
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(owner)
    .bind(session)
    .bind(question)
    .execute(&mut *tx)
    .await
    .unwrap();
    let queued: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM embedding_jobs WHERE session_id=$1)")
            .bind(session)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(!queued, "Do not embed before scoring");
    crate::audit::record(&mut tx,owner,None,session,"assessment.completed",json!({"competency_profile":{"competencies":[{"name":"Rust","percent":80}]},"questions":[{"score":8,"max_score":10}]})).await.unwrap();
    sqlx::query("UPDATE embedding_jobs SET available_at=now() WHERE session_id=$1")
        .bind(session)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    (session, question)
}
async fn ready_now(pool: &PgPool, owner: Uuid, session: Uuid) {
    let mut tx = interviewer_transaction(pool, owner)
        .await
        .unwrap();
    sqlx::query("UPDATE embedding_jobs SET available_at=now() WHERE session_id=$1")
        .bind(session)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
#[ignore = "requires disposable RAG_TEST_DATABASE_URL and localhost mock"]
async fn durable_embeddings_reuse_checkpoints_recover_and_remain_session_scoped() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("RAG_TEST_DATABASE_URL").expect("Use a disposable database"))
        .await
        .unwrap();
    let bypass: bool = sqlx::query_scalar(
        r#"
        SELECT rolsuper
        OR rolbypassrls
        FROM pg_roles
        WHERE rolname = current_user
        "#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!bypass);
    let mock = Mock {
        batches: Arc::new(Mutex::new(Vec::new())),
        fail_at: Arc::new(AtomicUsize::new(usize::MAX)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/embeddings", axum::routing::post(embeddings))
        .with_state(mock.clone());
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .unwrap();
    });
    let mut provider = AiProvider::new(
        format!("http://{address}"),
        "mock".into(),
        "mock".into(),
        "mock".into(),
        "test-embedding".into(),
    )
    .unwrap();
    provider.dimensions = 3;
    let tenant = owner(&pool).await;
    let (session, question) = setup(&pool, tenant).await;
    assert!(
        process_next(&pool, &provider)
            .await
            .unwrap()
    );
    assert_eq!(mock.batches.lock().unwrap().len(), 1);
    let indexed = search(&pool, tenant, session, &provider, "ownership", 0)
        .await
        .unwrap()
        .unwrap();
    assert!(indexed.total >= 4);
    assert_eq!(
        *mock
            .batches
            .lock()
            .unwrap()
            .last()
            .unwrap(),
        1,
        "Chat embeds only its query"
    );
    // An unchanged explicit retry reuses every saved vector, even across a worker restart.
    let before = mock.batches.lock().unwrap().len();
    let mut tx = interviewer_transaction(&pool, tenant)
        .await
        .unwrap();
    sqlx::query(
        r#"
        UPDATE embedding_jobs
        SET status = 'running',
            attempts = 1,
            claim_token = $2,
            lease_until = now() - interval '1 second'
        WHERE session_id = $1
        "#,
    )
    .bind(session)
    .bind(Uuid::new_v4())
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    process_next(&pool, &provider)
        .await
        .unwrap();
    assert_eq!(mock.batches.lock().unwrap().len(), before);
    // Late evidence dirties the index, while old final scores stay intact.
    let mut tx = interviewer_transaction(&pool, tenant)
        .await
        .unwrap();
    for i in 0..65 {
        crate::audit::record(
            &mut tx,
            tenant,
            None,
            session,
            "proctor.observed",
            json!({"kind":"focus_lost","test_number":i}),
        )
        .await
        .unwrap();
    }
    sqlx::query("UPDATE embedding_jobs SET available_at=now() WHERE session_id=$1")
        .bind(session)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        search(&pool, tenant, session, &provider, "focus", 0)
            .await
            .unwrap()
            .is_none()
    );
    mock.fail_at
        .store(before + 2, Ordering::SeqCst); // Save batch one, then fail batch two.
    process_next(&pool, &provider)
        .await
        .unwrap();
    let mut tx = interviewer_transaction(&pool, tenant)
        .await
        .unwrap();
    let (status, attempts): (String, i32) =
        sqlx::query_as("SELECT status,attempts FROM embedding_jobs WHERE session_id=$1")
            .bind(session)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!((status.as_str(), attempts), ("pending", 1));
    let checkpoint: i64 =
        sqlx::query_scalar("SELECT count(*) FROM evidence_embeddings WHERE session_id=$1")
            .bind(session)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(checkpoint >= 28);
    tx.commit().await.unwrap();
    mock.fail_at
        .store(usize::MAX, Ordering::SeqCst);
    ready_now(&pool, tenant, session).await;
    let request_start = mock.batches.lock().unwrap().len();
    process_next(&pool, &provider)
        .await
        .unwrap();
    let fresh_inputs: usize = mock.batches.lock().unwrap()[request_start..]
        .iter()
        .sum();
    assert!(fresh_inputs < 65, "Saved batch is reused after retry");
    // An answer revision re-embeds its changed chunk, not rubrics/events.
    let mut tx = interviewer_transaction(&pool, tenant)
        .await
        .unwrap();
    sqlx::query(
        r#"
        UPDATE interview_answers
        SET content = 'Revised candidate answer about ownership'
        WHERE session_id = $1
        AND question_id = $2
        "#,
    )
    .bind(session)
    .bind(question)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    ready_now(&pool, tenant, session).await;
    let request_start = mock.batches.lock().unwrap().len();
    process_next(&pool, &provider)
        .await
        .unwrap();
    assert_eq!(&mock.batches.lock().unwrap()[request_start..], &[1]);
    // Another candidate assigned this set shares only question/rubric vectors.
    let other_session = Uuid::new_v4();
    let mut tx = interviewer_transaction(&pool, tenant)
        .await
        .unwrap();
    sqlx::query(
        r#"
        INSERT INTO interview_sessions(id,
                interviewer_id,
                job_id,
                request_key,
                question_set_id,
                candidate_name,
                candidate_email,
                state,
                duration_minutes,
                interviewee_id)
        SELECT $1,
            interviewer_id,
            job_id,
            $2,
            question_set_id,
            'Other candidate',
            'other@example.test',
            'completed',
            30,
            interviewee_id
        FROM interview_sessions
        WHERE id = $3
        "#,
    )
    .bind(other_session)
    .bind(Uuid::new_v4())
    .bind(session)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO interview_answers(id,
                interviewer_id,
                session_id,
                question_id,
                content)
        VALUES ($1,
                $2,
                $3,
                $4,
                'SECRET_OTHER_CANDIDATE')
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(tenant)
    .bind(other_session)
    .bind(question)
    .execute(&mut *tx)
    .await
    .unwrap();
    crate::audit::record(
        &mut tx,
        tenant,
        None,
        other_session,
        "assessment.completed",
        json!({"score":5}),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    ready_now(&pool, tenant, other_session).await;
    let start = mock.batches.lock().unwrap().len();
    process_next(&pool, &provider)
        .await
        .unwrap();
    assert_eq!(
        &mock.batches.lock().unwrap()[start..],
        &[2],
        "Only the other answer and assessment need vectors"
    );
    let result = search(
        &pool,
        tenant,
        session,
        &provider,
        "SECRET_OTHER_CANDIDATE",
        0,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(
        result
            .sources
            .iter()
            .all(|(_, text)| !text.contains("SECRET_OTHER_CANDIDATE"))
    );
    let mut all = std::collections::HashSet::new();
    let mut page = 0;
    loop {
        let result = search(&pool, tenant, session, &provider, "ownership", page)
            .await
            .unwrap()
            .unwrap();
        if result.sources.is_empty() {
            assert_eq!(all.len(), result.total as usize);
            break;
        }
        for (id, _) in result.sources {
            assert!(all.insert(id));
        }
        page += 1;
    }
    let outsider = owner(&pool).await;
    assert!(
        search(&pool, outsider, session, &provider, "ownership", 0)
            .await
            .unwrap()
            .is_none()
    );
    let mut tx = interviewer_transaction(&pool, outsider)
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM evidence_embeddings WHERE session_id=$1")
            .bind(session)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(count, 0);
    tx.commit().await.unwrap();
    // Exhaustion is bounded; it never rolls completed scoring back.
    let mut tx = interviewer_transaction(&pool, tenant)
        .await
        .unwrap();
    crate::audit::record(
        &mut tx,
        tenant,
        None,
        session,
        "integrity.annotated",
        json!({"note":"New human review"}),
    )
    .await
    .unwrap();
    sqlx::query(
        r#"
        UPDATE embedding_jobs
        SET attempts = 3,
            available_at = now()
        WHERE session_id = $1
        "#,
    )
    .bind(session)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    mock.fail_at
        .store(mock.batches.lock().unwrap().len() + 1, Ordering::SeqCst);
    process_next(&pool, &provider)
        .await
        .unwrap();
    let mut tx = interviewer_transaction(&pool, tenant)
        .await
        .unwrap();
    let states: (String, String) = sqlx::query_as(
        r#"
        SELECT j.status,
            s.state
        FROM embedding_jobs j JOIN interview_sessions s ON s.id = j.session_id
        WHERE j.session_id = $1
        "#,
    )
    .bind(session)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(states, ("failed".into(), "completed".into()));
    tx.commit().await.unwrap();
    server.abort();
    pool.close().await;
}
