//! Job creation, unique human Job IDs and listing.
use crate::{
    Result,
    access::staff,
    audit::record as audit,
    error::{bad, conflict},
    pagination::{Page, paginated},
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;
#[derive(Deserialize)]
pub(crate) struct NewJob {
    job_code: String,
    title: String,
    description: String,
    request_key: Uuid,
    #[serde(default = "default_duration")]
    preparation_duration_minutes: i32,
    #[serde(default = "default_set_count")]
    question_set_count: i32,
}
fn default_set_count() -> i32 {
    1
}
fn default_duration() -> i32 {
    30
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
pub(crate) async fn create_job(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(input): Json<NewJob>,
) -> Result<(StatusCode, Json<Value>)> {
    if !(5..=180).contains(&input.preparation_duration_minutes) {
        return Err(bad("Interview duration must be 5–180 minutes"));
    }
    if !(1..=5).contains(&input.question_set_count) {
        return Err(bad("Question set count must be 1–5"));
    }
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
    let created = sqlx::query("INSERT INTO jobs(id,interviewer_id,request_key,title,description,job_code,preparation_duration_minutes,question_set_count) VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT DO NOTHING")
        .bind(id).bind(recruiter).bind(input.request_key).bind(title).bind(description).bind(&job_code).bind(input.preparation_duration_minutes).bind(input.question_set_count).execute(&mut *tx).await?.rows_affected() == 1;
    if created {
        crate::generation::bank::replenish(
            &mut tx,
            recruiter,
            id,
            input.preparation_duration_minutes,
        )
        .await?;
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
    let result: Value = sqlx::query_scalar("SELECT jsonb_build_object('id',j.id,'job_code',j.job_code,'title',j.title,'description',j.description,'created_at',j.created_at,'preparation_duration_minutes',j.preparation_duration_minutes,'question_set_count',j.question_set_count) FROM jobs j WHERE j.interviewer_id=$1 AND j.request_key=$2")
        .bind(recruiter).bind(input.request_key).fetch_optional(&mut *tx).await?.ok_or(conflict("A job with this Job ID already exists"))?;
    if result["job_code"] != job_code
        || result["title"] != title
        || result["description"] != description
        || result["question_set_count"] != input.question_set_count
        || result["preparation_duration_minutes"] != input.preparation_duration_minutes
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
pub(crate) async fn list_jobs(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    let limit = page.limit()?;
    let (recruiter, mut tx) = staff(&pool, &headers).await?;
    let items: Vec<Value> = sqlx::query_scalar("SELECT jsonb_build_object('id',j.id,'job_code',j.job_code,'title',j.title,'description',j.description,'created_at',j.created_at,'preparation_duration_minutes',j.preparation_duration_minutes,'question_set_count',j.question_set_count,'session_count',(SELECT count(*) FROM interview_sessions s WHERE s.job_id=j.id)) FROM jobs j WHERE j.interviewer_id=$1 AND ($2::uuid IS NULL OR (j.created_at,j.id)<(SELECT created_at,id FROM jobs WHERE id=$2)) ORDER BY j.created_at DESC,j.id DESC LIMIT $3")
        .bind(recruiter).bind(page.cursor).bind(limit+1).fetch_all(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(paginated(items, limit)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn job_codes_are_normalized_and_bounded() {
        assert_eq!(normalize_job_code("  be-2026_01 ").unwrap(), "BE-2026_01");
        for code in ["", " ", "_BAD", "BAD ID", "../BAD", "é123"] {
            assert!(normalize_job_code(code).is_err());
        }
        assert!(normalize_job_code(&"A".repeat(65)).is_err());
    }
}
