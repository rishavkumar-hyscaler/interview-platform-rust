//! Interview orchestration. Every staff operation verifies membership before tenant data access.
use crate::infrastructure::interviewer_transaction;
use crate::invitations::InvitationCipher;
use crate::{ApiError, Result, authenticate, digest, new_token};
use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

pub fn routes() -> Router<PgPool> {
    Router::new()
        .route("/api/v1/summary", get(summary))
        .route("/api/v1/jobs", get(list_jobs).post(create_job))
        .route("/api/v1/jobs/{job}/invitations", post(invite))
        .route("/api/v1/sessions", get(list_sessions))
        .route("/api/v1/sessions/{session}", get(session_detail))
        .route(
            "/api/v1/sessions/{session}/invitation",
            post(retrieve_invitation),
        )
        .route("/api/v1/audit", get(audit_log))
        .route("/api/v1/candidate/session", get(candidate_session))
        .route(
            "/api/v1/candidate/session/{action}",
            post(candidate_transition),
        )
}
fn bad(message: &'static str) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message)
}
fn missing() -> ApiError {
    ApiError(StatusCode::NOT_FOUND, "Resource not found")
}
fn conflict(message: &'static str) -> ApiError {
    ApiError(StatusCode::CONFLICT, message)
}
pub(crate) async fn staff<'a>(
    pool: &'a PgPool,
    headers: &HeaderMap,
) -> Result<(Uuid, Transaction<'a, Postgres>)> {
    let (user, _) = authenticate(pool, headers).await?;
    Ok((user, interviewer_transaction(pool, user).await?))
}
pub(crate) async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    recruiter: Uuid,
    actor: Option<Uuid>,
    entity: Uuid,
    action: &str,
    metadata: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_events(id,interviewer_id,actor_id,entity_id,action,metadata) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(Uuid::new_v4()).bind(recruiter).bind(actor).bind(entity).bind(action).bind(metadata).execute(&mut **tx).await?;
    Ok(())
}
#[derive(Deserialize)]
struct Page {
    cursor: Option<Uuid>,
    limit: Option<i64>,
    entity_id: Option<Uuid>,
}
impl Page {
    fn limit(&self) -> Result<i64> {
        match self.limit.unwrap_or(20) {
            n @ 1..=100 => Ok(n),
            _ => Err(bad("Limit must be between 1 and 100")),
        }
    }
}
fn paginated(mut items: Vec<Value>, limit: i64) -> Value {
    let more = items.len() > limit as usize;
    items.truncate(limit as usize);
    let cursor = if more {
        items.last().and_then(|item| item.get("id")).cloned()
    } else {
        None
    };
    json!({"items":items,"next_cursor":cursor})
}
#[derive(Deserialize)]
struct NewJob {
    job_code: String,
    title: String,
    description: String,
    request_key: Uuid,
}
fn normalize_job_code(value: &str) -> Result<String> {
    let code = value.trim().to_ascii_uppercase();
    if !(1..=64).contains(&code.len())
        || !code.as_bytes()[0].is_ascii_alphanumeric()
        || !code
            .bytes()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == b'-' || ch == b'_')
    {
        return Err(bad(
            "Job ID must be 1–64 characters: letters, numbers, hyphens or underscores; start with a letter or number",
        ));
    }
    Ok(code)
}
async fn create_job(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(input): Json<NewJob>,
) -> Result<(StatusCode, Json<Value>)> {
    let job_code = normalize_job_code(&input.job_code)?;
    let title = input.title.trim();
    let description = input.description.trim();
    if title.contains('\0')
        || description.contains('\0')
        || !(1..=160).contains(&title.chars().count())
        || !(20..=20000).contains(&description.chars().count())
    {
        return Err(bad(
            "Title must be 1–160 characters and job description 20–20000 characters",
        ));
    }
    let (user, mut tx) = staff(&pool, &headers).await?;
    let recruiter = user;
    let id = Uuid::new_v4();
    let created = sqlx::query("INSERT INTO jobs(id,interviewer_id,request_key,title,created_by,description,job_code) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT DO NOTHING")
        .bind(id).bind(recruiter).bind(input.request_key).bind(title).bind(user).bind(description).bind(&job_code).execute(&mut *tx).await?.rows_affected() == 1;
    if created {
        audit(
            &mut tx,
            recruiter,
            Some(user),
            id,
            "job.created",
            json!({"version":1,"job_code":job_code}),
        )
        .await?;
    }
    let result: Value = sqlx::query_scalar("SELECT jsonb_build_object('id',j.id,'job_code',j.job_code,'title',j.title,'description',j.description,'created_at',j.created_at) FROM jobs j WHERE j.interviewer_id=$1 AND j.request_key=$2")
        .bind(recruiter).bind(input.request_key).fetch_optional(&mut *tx).await?.ok_or(conflict("A job with this Job ID already exists"))?;
    if result["job_code"] != job_code
        || result["title"] != title
        || result["description"] != description
    {
        return Err(conflict(
            "Request key was already used with different job details",
        ));
    }
    tx.commit().await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(result),
    ))
}
async fn list_jobs(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    let limit = page.limit()?;
    let (recruiter, mut tx) = staff(&pool, &headers).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',j.id,'job_code',j.job_code,'title',j.title,'description',j.description,'created_at',j.created_at,'session_count',(SELECT count(*) FROM interview_sessions s WHERE s.job_id=j.id)) FROM jobs j WHERE j.interviewer_id=$1 AND ($2::uuid IS NULL OR (j.created_at,j.id)<(SELECT created_at,id FROM jobs WHERE id=$2)) ORDER BY j.created_at DESC,j.id DESC LIMIT $3")
        .bind(recruiter).bind(page.cursor).bind(limit+1).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(paginated(items, limit)))
}
#[derive(Deserialize)]
struct Invitation {
    candidate_name: String,
    candidate_email: String,
    duration_minutes: i32,
    request_key: Uuid,
}
async fn invite(
    State(pool): State<PgPool>,
    Path(job): Path<Uuid>,
    Extension(automatic): Extension<crate::generation::AutomaticPreparation>,
    headers: HeaderMap,
    Json(input): Json<Invitation>,
) -> Result<(StatusCode, Json<Value>)> {
    let name = input.candidate_name.trim();
    let email = input.candidate_email.trim().to_lowercase();
    if name.contains('\0')
        || email.contains('\0')
        || !(1..=120).contains(&name.chars().count())
        || email.len() > 254
        || !email.contains('@')
        || email.chars().any(char::is_whitespace)
        || !(5..=180).contains(&input.duration_minutes)
    {
        return Err(bad(
            "Provide a candidate name, email, and a duration of 5–180 minutes",
        ));
    }
    let (user, mut tx) = staff(&pool, &headers).await?;
    let recruiter = user;
    let _job: Uuid = sqlx::query_scalar("SELECT id FROM jobs WHERE interviewer_id=$1 AND id=$2")
        .bind(recruiter)
        .bind(job)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(missing)?;
    sqlx::query("INSERT INTO users(id,email,role) VALUES($1,$2,'interviewee') ON CONFLICT(email,role) DO NOTHING")
        .bind(Uuid::new_v4()).bind(&email).execute(&mut *tx).await?;
    let interviewee: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email=$1 AND role='interviewee'")
            .bind(&email)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(conflict(
                "This email belongs to an interviewer. Use the interviewee's email address.",
            ))?;
    let id = Uuid::new_v4();
    let inserted = sqlx::query("INSERT INTO interview_sessions(id,interviewer_id,job_id,request_key,candidate_name,candidate_email,duration_minutes,created_by,interviewee_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(interviewer_id,request_key) DO NOTHING")
        .bind(id).bind(recruiter).bind(job).bind(input.request_key).bind(name).bind(&email).bind(input.duration_minutes).bind(user).bind(interviewee).execute(&mut *tx).await?.rows_affected()==1;
    if inserted {
        crate::generation::prepare_automatically(&mut tx, recruiter, id, automatic.0).await?;
    }
    let result: Value = sqlx::query_scalar("SELECT to_jsonb(s)-'request_key'-'created_by' FROM interview_sessions s WHERE interviewer_id=$1 AND request_key=$2")
        .bind(recruiter).bind(input.request_key).fetch_one(&mut *tx).await?;
    if result["job_id"] != job.to_string()
        || result["candidate_name"] != name
        || result["candidate_email"] != email
        || result["duration_minutes"] != input.duration_minutes
    {
        return Err(conflict(
            "Request key was already used with different invitation details",
        ));
    }
    let token = if inserted {
        let token = new_token();
        let sealed = cipher()?
            .seal(&token, recruiter, id)
            .map_err(|_| link_unavailable())?;
        sqlx::query("INSERT INTO auth_sessions(token_hash,user_id,interviewer_id,interview_session_id,expires_at,sealed_token) SELECT $1,interviewee_id,interviewer_id,id,expires_at,$3 FROM interview_sessions WHERE id=$2")
            .bind(digest(&token)).bind(id).bind(sealed).execute(&mut *tx).await?;
        audit(
            &mut tx,
            recruiter,
            Some(user),
            id,
            "session.invited",
            json!({"job_id":job}),
        )
        .await?;
        Some(token)
    } else {
        let session_id: Uuid = result["id"]
            .as_str()
            .ok_or_else(link_unavailable)?
            .parse()
            .map_err(|_| link_unavailable())?;
        let sealed: Option<String> = sqlx::query_scalar(
            "SELECT sealed_token FROM auth_sessions WHERE interview_session_id=$1 AND interviewer_id=$2",
        )
        .bind(session_id)
        .bind(recruiter)
        .fetch_one(&mut *tx)
        .await?;
        sealed
            .map(|value| {
                cipher()?
                    .open(&value, recruiter, session_id)
                    .map_err(|_| link_unavailable())
            })
            .transpose()?
    };
    tx.commit().await?;
    Ok((
        if inserted {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(json!({"session":result,"invitation_token":token})),
    ))
}
fn link_unavailable() -> ApiError {
    ApiError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Invitation link could not be recovered. Check the configured encryption key.",
    )
}
fn cipher() -> Result<InvitationCipher> {
    InvitationCipher::from_environment().map_err(|_| link_unavailable())
}
async fn retrieve_invitation(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (user, mut tx) = staff(&pool, &headers).await?;
    let recruiter = user;
    let session = sqlx::query("SELECT state, expires_at>now() AND least(expires_at,coalesce(deadline_at,expires_at))>now() AS active FROM interview_sessions WHERE id=$1 AND interviewer_id=$2 FOR UPDATE")
        .bind(id).bind(recruiter).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    if !session.get::<bool, _>("active")
        || !matches!(
            session.get::<String, _>("state").as_str(),
            "invited" | "ready" | "in_progress"
        )
    {
        return Err(conflict(
            "Invitation links are unavailable for expired or submitted sessions",
        ));
    }
    let row = sqlx::query("SELECT token_hash,sealed_token,expires_at::text AS expires_at FROM auth_sessions WHERE interview_session_id=$1 AND interviewer_id=$2 AND expires_at>now() FOR UPDATE")
        .bind(id).bind(recruiter).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    let sealed: Option<String> = row.get("sealed_token");
    let replaced_legacy_link = sealed.is_none();
    // Historical hashes cannot be reversed. Explicit recovery reissues legacy links
    // atomically; new invitations always return the exact original token.
    let token = if let Some(sealed) = sealed {
        let token = cipher()?
            .open(&sealed, recruiter, id)
            .map_err(|_| link_unavailable())?;
        if digest(&token) != row.get::<String, _>("token_hash") {
            return Err(link_unavailable());
        }
        token
    } else {
        if session.get::<String, _>("state") != "invited" {
            return Err(conflict(
                "This legacy session has already been accepted; its original link cannot be recovered",
            ));
        }
        let token = new_token();
        let sealed = cipher()?
            .seal(&token, recruiter, id)
            .map_err(|_| link_unavailable())?;
        sqlx::query("UPDATE auth_sessions SET token_hash=$1,sealed_token=$2 WHERE interview_session_id=$3 AND interviewer_id=$4")
            .bind(digest(&token)).bind(sealed).bind(id).bind(recruiter).execute(&mut *tx).await?;
        token
    };
    audit(
        &mut tx,
        recruiter,
        Some(user),
        id,
        if replaced_legacy_link {
            "invitation.link_reissued"
        } else {
            "invitation.link_retrieved"
        },
        json!({"replaced_legacy_link":replaced_legacy_link}),
    )
    .await?;
    let expires_at: String = row.get("expires_at");
    tx.commit().await?;
    Ok(Json(
        json!({"invitation_token":token,"expires_at":expires_at,"replaced_legacy_link":replaced_legacy_link}),
    ))
}
async fn list_sessions(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    let limit = page.limit()?;
    let (recruiter, mut tx) = staff(&pool, &headers).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT (to_jsonb(s)-'request_key'-'created_by') || jsonb_build_object('job_title',j.title,'job_code',j.job_code) FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.interviewer_id=$1 AND ($2::uuid IS NULL OR (s.created_at,s.id)<(SELECT created_at,id FROM interview_sessions WHERE id=$2)) ORDER BY s.created_at DESC,s.id DESC LIMIT $3")
        .bind(recruiter).bind(page.cursor).bind(limit+1).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(paginated(items, limit)))
}
async fn session_detail(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (recruiter, mut tx) = staff(&pool, &headers).await?;
    let result: Value=sqlx::query_scalar("SELECT (to_jsonb(s)-'request_key'-'created_by') || jsonb_build_object('job_title',j.title,'job_code',j.job_code,'description',j.description,'question_count',(SELECT count(*) FROM interview_questions q WHERE q.session_id=s.id),'answer_count',(SELECT count(*) FROM interview_questions q WHERE q.session_id=s.id AND (EXISTS(SELECT 1 FROM interview_answers a WHERE a.question_id=q.id AND length(btrim(a.content))>0) OR EXISTS(SELECT 1 FROM audit_events m WHERE m.entity_id=q.session_id AND m.action='answer.recorded' AND m.metadata->>'question_id'=q.id::text))),'invitation_recoverable',coalesce((SELECT sealed_token IS NOT NULL FROM auth_sessions c WHERE c.interview_session_id=s.id),false),'invitation_active',s.state IN ('invited','ready','in_progress') AND least(s.expires_at,coalesce(s.deadline_at,s.expires_at))>now()) FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.id=$1 AND s.interviewer_id=$2")
        .bind(id).bind(recruiter).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn audit_log(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    let limit = page.limit()?;
    let (recruiter, mut tx) = staff(&pool, &headers).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(a) FROM audit_events a WHERE interviewer_id=$1 AND ($2::uuid IS NULL OR entity_id=$2) AND ($3::uuid IS NULL OR (created_at,id)<(SELECT created_at,id FROM audit_events WHERE id=$3)) ORDER BY created_at DESC,id DESC LIMIT $4")
        .bind(recruiter).bind(page.entity_id).bind(page.cursor).bind(limit+1).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(paginated(items, limit)))
}
pub(crate) async fn candidate<'a>(
    pool: &'a PgPool,
    headers: &HeaderMap,
) -> Result<(Uuid, Uuid, Transaction<'a, Postgres>)> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| v.len() == 64)
        .ok_or_else(crate::unauthorized)?;
    let row=sqlx::query("SELECT a.interviewer_id,a.interview_session_id AS session_id FROM auth_sessions a JOIN users u ON u.id=a.user_id WHERE a.token_hash=$1 AND a.expires_at>now() AND a.interview_session_id IS NOT NULL AND u.role='interviewee'")
        .bind(digest(token)).fetch_optional(pool).await?.ok_or_else(crate::unauthorized)?;
    let recruiter: Uuid = row.get("interviewer_id");
    let id: Uuid = row.get("session_id");
    Ok((
        recruiter,
        id,
        interviewer_transaction(pool, recruiter).await?,
    ))
}
pub(crate) async fn candidate_view(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Value> {
    // Deliberately excludes rubrics, other candidates, email, and internal audit data.
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'role','interviewee','interviewee_id',s.interviewee_id,'job_title',j.title,'job_code',j.job_code,'state',CASE WHEN s.state IN ('invited','ready','in_progress') AND least(s.expires_at,coalesce(s.deadline_at,s.expires_at))<=now() THEN CASE WHEN s.state='in_progress' THEN 'submitted' ELSE 'expired' END ELSE s.state END,'duration_minutes',s.duration_minutes,'expires_at',s.expires_at,'deadline_at',s.deadline_at,'server_time',now(),'started_at',s.started_at,'submitted_at',s.submitted_at,'generation_status',s.generation_status,'answer_count',(SELECT count(*) FROM interview_questions q WHERE q.session_id=s.id AND (EXISTS(SELECT 1 FROM interview_answers a WHERE a.question_id=q.id AND length(btrim(a.content))>0) OR EXISTS(SELECT 1 FROM audit_events m WHERE m.entity_id=q.session_id AND m.action='answer.recorded' AND m.metadata->>'question_id'=q.id::text))),'question_count',(SELECT count(*) FROM interview_questions WHERE session_id=s.id)) FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.id=$1")
        .bind(id).fetch_one(&mut **tx).await?)
}
async fn candidate_session(
    State(pool): State<PgPool>,
    Extension(automatic): Extension<crate::generation::AutomaticPreparation>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (owner, id, mut tx) = candidate(&pool, &headers).await?;
    crate::infrastructure::finalize_due_session(&mut tx, owner, id).await?;
    crate::generation::prepare_automatically(&mut tx, owner, id, automatic.0).await?;
    let mut result = candidate_view(&mut tx, id).await?;
    result["preparation_available"] = json!(automatic.0);
    tx.commit().await?;
    Ok(Json(result))
}
fn next_state(current: &str, action: &str) -> Result<&'static str> {
    match (current, action) {
        ("invited" | "ready", "ready") => Ok("ready"),
        ("ready" | "in_progress", "start") => Ok("in_progress"),
        ("in_progress" | "submitted", "submit") => Ok("submitted"),
        _ => Err(conflict("This session transition is not allowed")),
    }
}
async fn candidate_transition(
    State(pool): State<PgPool>,
    Extension(automatic): Extension<crate::generation::AutomaticPreparation>,
    Path(action): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (recruiter, id, mut tx) = candidate(&pool, &headers).await?;
    crate::infrastructure::finalize_due_session(&mut tx, recruiter, id).await?;
    let row=sqlx::query("SELECT state,least(expires_at,coalesce(deadline_at,expires_at))>now() AS active FROM interview_sessions WHERE id=$1 FOR UPDATE")
        .bind(id).fetch_one(&mut *tx).await?;
    let current: String = row.get("state");
    let next = next_state(&current, &action)?;
    if next != current {
        if !row.get::<bool, _>("active") {
            return Err(conflict("Session has expired"));
        }
        if action == "start" {
            let count: i64 =
                sqlx::query_scalar("SELECT count(*) FROM interview_questions WHERE session_id=$1")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await?;
            if count == 0 {
                return Err(conflict("Questions are not prepared yet"));
            }
        }
        sqlx::query("UPDATE interview_sessions SET state=$2,started_at=CASE WHEN $2='in_progress' THEN now() ELSE started_at END,deadline_at=CASE WHEN $2='in_progress' THEN least(expires_at,now()+duration_minutes*interval '1 minute') ELSE deadline_at END,submitted_at=CASE WHEN $2='submitted' THEN now() ELSE submitted_at END WHERE id=$1")
            .bind(id).bind(next).execute(&mut *tx).await?;
        audit(
            &mut tx,
            recruiter,
            None,
            id,
            &format!("session.{next}"),
            json!({"actor":"candidate","previous_state":current}),
        )
        .await?;
    }
    if action == "ready" {
        crate::generation::prepare_automatically(&mut tx, recruiter, id, automatic.0).await?;
    }
    let mut result = candidate_view(&mut tx, id).await?;
    result["preparation_available"] = json!(automatic.0);
    tx.commit().await?;
    Ok(Json(result))
}
async fn summary(State(pool): State<PgPool>, headers: HeaderMap) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers).await?;
    let result: Value=sqlx::query_scalar("SELECT jsonb_build_object('jobs',(SELECT count(*) FROM jobs),'sessions',count(*),'active',count(*) FILTER(WHERE state IN ('ready','in_progress') AND least(expires_at,coalesce(deadline_at,expires_at))>now()),'reviewable',count(*) FILTER(WHERE state IN ('submitted','scoring','completed'))) FROM interview_sessions")
        .fetch_one(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transitions_are_ordered_and_retries_are_safe() {
        assert_eq!(next_state("invited", "ready").unwrap(), "ready");
        assert_eq!(next_state("ready", "ready").unwrap(), "ready");
        assert_eq!(next_state("ready", "start").unwrap(), "in_progress");
        assert_eq!(next_state("in_progress", "start").unwrap(), "in_progress");
        assert_eq!(next_state("in_progress", "submit").unwrap(), "submitted");
        assert_eq!(next_state("submitted", "submit").unwrap(), "submitted");
        for (state, action) in [
            ("invited", "submit"),
            ("invited", "start"),
            ("submitted", "start"),
            ("expired", "ready"),
            ("completed", "submit"),
            ("ready", "completed"),
        ] {
            assert!(next_state(state, action).is_err());
        }
    }
    #[test]
    fn job_codes_are_normalized_and_bounded() {
        assert_eq!(normalize_job_code("  be-2026_01 ").unwrap(), "BE-2026_01");
        for code in ["", " ", "_BAD", "BAD ID", "../BAD", "é123"] {
            assert!(normalize_job_code(code).is_err());
        }
        assert!(normalize_job_code(&"A".repeat(65)).is_err());
    }
}
