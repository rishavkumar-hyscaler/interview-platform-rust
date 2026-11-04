//! Interview orchestration. Every staff operation verifies membership before tenant data access.
use crate::infrastructure::tenant_transaction;
use crate::invitations::InvitationCipher;
use crate::{ApiError, Result, authenticate, digest, new_token};
use axum::{
    Json, Router,
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
        .route("/api/v1/organizations/{org}/summary", get(summary))
        .route(
            "/api/v1/organizations/{org}/jobs",
            get(list_jobs).post(create_job),
        )
        .route(
            "/api/v1/organizations/{org}/jobs/{job}/invitations",
            post(invite),
        )
        .route("/api/v1/organizations/{org}/sessions", get(list_sessions))
        .route(
            "/api/v1/organizations/{org}/sessions/{session}",
            get(session_detail),
        )
        .route(
            "/api/v1/organizations/{org}/sessions/{session}/invitation",
            post(retrieve_invitation),
        )
        .route("/api/v1/organizations/{org}/audit", get(audit_log))
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
async fn staff<'a>(
    pool: &'a PgPool,
    headers: &HeaderMap,
    org: Uuid,
) -> Result<(Uuid, Transaction<'a, Postgres>)> {
    let (user, _) = authenticate(pool, headers).await?;
    let mut tx = tenant_transaction(pool, org).await?;
    let role: String =
        sqlx::query_scalar("SELECT role FROM memberships WHERE organization_id=$1 AND user_id=$2")
            .bind(org)
            .bind(user)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
    if role != "recruiter" {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Recruiter access is required",
        ));
    }
    Ok((user, tx))
}
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    org: Uuid,
    actor: Option<Uuid>,
    entity: Uuid,
    action: &str,
    metadata: Value,
) -> Result<()> {
    sqlx::query("INSERT INTO audit_events(id,organization_id,actor_id,entity_id,action,metadata) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(Uuid::new_v4()).bind(org).bind(actor).bind(entity).bind(action).bind(metadata).execute(&mut **tx).await?;
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
    title: String,
    description: String,
    request_key: Uuid,
}
async fn create_job(
    State(pool): State<PgPool>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<NewJob>,
) -> Result<(StatusCode, Json<Value>)> {
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
    let (user, mut tx) = staff(&pool, &headers, org).await?;
    let id = Uuid::new_v4();
    let created = sqlx::query("INSERT INTO jobs(id,organization_id,request_key,title,created_by) VALUES($1,$2,$3,$4,$5) ON CONFLICT(organization_id,request_key) DO NOTHING")
        .bind(id).bind(org).bind(input.request_key).bind(title).bind(user).execute(&mut *tx).await?.rows_affected() == 1;
    if created {
        sqlx::query("INSERT INTO job_description_versions(id,organization_id,job_id,version,description) VALUES($1,$2,$3,1,$4)")
            .bind(Uuid::new_v4()).bind(org).bind(id).bind(description).execute(&mut *tx).await?;
        audit(
            &mut tx,
            org,
            Some(user),
            id,
            "job.created",
            json!({"version":1}),
        )
        .await?;
    }
    let result: Value = sqlx::query_scalar("SELECT jsonb_build_object('id',j.id,'title',j.title,'description',d.description,'jd_version_id',d.id,'version',d.version,'created_at',j.created_at) FROM jobs j JOIN job_description_versions d ON d.job_id=j.id AND d.version=1 WHERE j.organization_id=$1 AND j.request_key=$2")
        .bind(org).bind(input.request_key).fetch_one(&mut *tx).await?;
    if result["title"] != title || result["description"] != description {
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
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    let limit = page.limit()?;
    let (_, mut tx) = staff(&pool, &headers, org).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',j.id,'title',j.title,'description',d.description,'jd_version_id',d.id,'version',d.version,'created_at',j.created_at,'session_count',(SELECT count(*) FROM interview_sessions s WHERE s.job_id=j.id)) FROM jobs j JOIN job_description_versions d ON d.job_id=j.id AND d.version=(SELECT max(version) FROM job_description_versions WHERE job_id=j.id) WHERE j.organization_id=$1 AND ($2::uuid IS NULL OR (j.created_at,j.id)<(SELECT created_at,id FROM jobs WHERE id=$2)) ORDER BY j.created_at DESC,j.id DESC LIMIT $3")
        .bind(org).bind(page.cursor).bind(limit+1).fetch_all(&mut *tx).await?;
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
    Path((org, job)): Path<(Uuid, Uuid)>,
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
    let (user, mut tx) = staff(&pool, &headers, org).await?;
    let jd: Uuid = sqlx::query_scalar("SELECT id FROM job_description_versions WHERE organization_id=$1 AND job_id=$2 ORDER BY version DESC LIMIT 1")
        .bind(org).bind(job).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    let id = Uuid::new_v4();
    let inserted = sqlx::query("INSERT INTO interview_sessions(id,organization_id,job_id,jd_version_id,request_key,candidate_name,candidate_email,duration_minutes,created_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT(organization_id,request_key) DO NOTHING")
        .bind(id).bind(org).bind(job).bind(jd).bind(input.request_key).bind(name).bind(&email).bind(input.duration_minutes).bind(user).execute(&mut *tx).await?.rows_affected()==1;
    let result: Value = sqlx::query_scalar("SELECT to_jsonb(s)-'request_key'-'created_by' FROM interview_sessions s WHERE organization_id=$1 AND request_key=$2")
        .bind(org).bind(input.request_key).fetch_one(&mut *tx).await?;
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
            .seal(&token, org, id)
            .map_err(|_| link_unavailable())?;
        sqlx::query("INSERT INTO candidate_access(token_hash,organization_id,session_id,expires_at,sealed_token) SELECT $1,organization_id,id,expires_at,$3 FROM interview_sessions WHERE id=$2")
            .bind(digest(&token)).bind(id).bind(sealed).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO background_jobs(id,organization_id,session_id,kind,available_at) SELECT $1,organization_id,id,'expire_session',expires_at FROM interview_sessions WHERE id=$2")
            .bind(Uuid::new_v4()).bind(id).execute(&mut *tx).await?;
        audit(
            &mut tx,
            org,
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
            "SELECT sealed_token FROM candidate_access WHERE session_id=$1 AND organization_id=$2",
        )
        .bind(session_id)
        .bind(org)
        .fetch_one(&mut *tx)
        .await?;
        sealed
            .map(|value| {
                cipher()?
                    .open(&value, org, session_id)
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
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (user, mut tx) = staff(&pool, &headers, org).await?;
    let session = sqlx::query("SELECT state, expires_at>now() AND least(expires_at,coalesce(deadline_at,expires_at))>now() AS active FROM interview_sessions WHERE id=$1 AND organization_id=$2 FOR UPDATE")
        .bind(id).bind(org).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
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
    let row = sqlx::query("SELECT token_hash,sealed_token,expires_at::text AS expires_at FROM candidate_access WHERE session_id=$1 AND organization_id=$2 AND expires_at>now() FOR UPDATE")
        .bind(id).bind(org).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    let sealed: Option<String> = row.get("sealed_token");
    let replaced_legacy_link = sealed.is_none();
    // Historical hashes cannot be reversed. Explicit recovery reissues legacy links
    // atomically; new invitations always return the exact original token.
    let token = if let Some(sealed) = sealed {
        let token = cipher()?
            .open(&sealed, org, id)
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
            .seal(&token, org, id)
            .map_err(|_| link_unavailable())?;
        sqlx::query("UPDATE candidate_access SET token_hash=$1,sealed_token=$2 WHERE session_id=$3 AND organization_id=$4")
            .bind(digest(&token)).bind(sealed).bind(id).bind(org).execute(&mut *tx).await?;
        token
    };
    audit(
        &mut tx,
        org,
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
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    let limit = page.limit()?;
    let (_, mut tx) = staff(&pool, &headers, org).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT (to_jsonb(s)-'request_key'-'created_by') || jsonb_build_object('job_title',j.title) FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.organization_id=$1 AND ($2::uuid IS NULL OR (s.created_at,s.id)<(SELECT created_at,id FROM interview_sessions WHERE id=$2)) ORDER BY s.created_at DESC,s.id DESC LIMIT $3")
        .bind(org).bind(page.cursor).bind(limit+1).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(paginated(items, limit)))
}
async fn session_detail(
    State(pool): State<PgPool>,
    Path((org, id)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers, org).await?;
    let result: Value=sqlx::query_scalar("SELECT (to_jsonb(s)-'request_key'-'created_by') || jsonb_build_object('job_title',j.title,'description',d.description,'jd_version',d.version,'question_count',(SELECT count(*) FROM interview_questions q WHERE q.session_id=s.id),'answer_count',(SELECT count(*) FROM interview_answers a WHERE a.session_id=s.id),'invitation_recoverable',coalesce((SELECT sealed_token IS NOT NULL FROM candidate_access c WHERE c.session_id=s.id),false),'invitation_active',s.state IN ('invited','ready','in_progress') AND least(s.expires_at,coalesce(s.deadline_at,s.expires_at))>now()) FROM interview_sessions s JOIN jobs j ON j.id=s.job_id JOIN job_description_versions d ON d.id=s.jd_version_id WHERE s.id=$1 AND s.organization_id=$2")
        .bind(id).bind(org).fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn audit_log(
    State(pool): State<PgPool>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    let limit = page.limit()?;
    let (_, mut tx) = staff(&pool, &headers, org).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(a) FROM audit_events a WHERE organization_id=$1 AND ($2::uuid IS NULL OR entity_id=$2) AND ($3::uuid IS NULL OR (created_at,id)<(SELECT created_at,id FROM audit_events WHERE id=$3)) ORDER BY created_at DESC,id DESC LIMIT $4")
        .bind(org).bind(page.entity_id).bind(page.cursor).bind(limit+1).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(paginated(items, limit)))
}
async fn candidate<'a>(
    pool: &'a PgPool,
    headers: &HeaderMap,
) -> Result<(Uuid, Uuid, Transaction<'a, Postgres>)> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| v.len() == 64)
        .ok_or_else(crate::unauthorized)?;
    let row=sqlx::query("SELECT organization_id,session_id FROM candidate_access WHERE token_hash=$1 AND expires_at>now()")
        .bind(digest(token)).fetch_optional(pool).await?.ok_or_else(crate::unauthorized)?;
    let org: Uuid = row.get("organization_id");
    let id: Uuid = row.get("session_id");
    Ok((org, id, tenant_transaction(pool, org).await?))
}
async fn candidate_view(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Value> {
    // Deliberately excludes rubrics, other candidates, email, and internal audit data.
    Ok(sqlx::query_scalar("SELECT jsonb_build_object('id',s.id,'job_title',j.title,'state',CASE WHEN s.state IN ('invited','ready','in_progress') AND least(s.expires_at,coalesce(s.deadline_at,s.expires_at))<=now() THEN 'expired' ELSE s.state END,'duration_minutes',s.duration_minutes,'expires_at',s.expires_at,'deadline_at',s.deadline_at,'server_time',now(),'question_count',(SELECT count(*) FROM interview_questions WHERE session_id=s.id)) FROM interview_sessions s JOIN jobs j ON j.id=s.job_id WHERE s.id=$1")
        .bind(id).fetch_one(&mut **tx).await?)
}
async fn candidate_session(State(pool): State<PgPool>, headers: HeaderMap) -> Result<Json<Value>> {
    let (_, id, mut tx) = candidate(&pool, &headers).await?;
    let result = candidate_view(&mut tx, id).await?;
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
    Path(action): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (org, id, mut tx) = candidate(&pool, &headers).await?;
    sqlx::query("SELECT id FROM background_jobs WHERE session_id=$1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
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
        if action == "start" {
            sqlx::query("UPDATE background_jobs SET available_at=(SELECT deadline_at FROM interview_sessions WHERE id=$1) WHERE session_id=$1 AND kind='expire_session' AND status='pending'").bind(id).execute(&mut *tx).await?;
        }
        audit(
            &mut tx,
            org,
            None,
            id,
            &format!("session.{next}"),
            json!({"actor":"candidate","previous_state":current}),
        )
        .await?;
    }
    let result = candidate_view(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(result))
}
async fn summary(
    State(pool): State<PgPool>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    let (_, mut tx) = staff(&pool, &headers, org).await?;
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
}
