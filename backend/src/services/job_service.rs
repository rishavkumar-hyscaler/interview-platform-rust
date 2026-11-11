//! Job rules and transaction boundaries, independent of Axum and HTTP DTOs.
use crate::{
    Result,
    audit::record as audit,
    error::{bad, conflict},
    infrastructure::interviewer_transaction,
    models::job::{Job, JobFilters, JobListItem, NewJob},
    repositories::job_repository,
};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

pub(crate) struct CreateJob {
    pub job_code: String,
    pub title: String,
    pub description: String,
    pub request_key: Uuid,
    pub preparation_duration_minutes: i32,
    pub question_set_count: i32,
}
pub(crate) struct CreatedJob {
    pub job: Job,
    pub created: bool,
}
pub(crate) struct JobPage {
    pub total: i64,
    pub items: Vec<JobListItem>,
    pub next_cursor: Option<Uuid>,
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

pub(crate) async fn create(
    pool: &PgPool,
    interviewer: Uuid,
    input: CreateJob,
) -> Result<CreatedJob> {
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
    let new_job = NewJob {
        id: Uuid::new_v4(),
        interviewer_id: interviewer,
        request_key: input.request_key,
        job_code,
        title: title.to_owned(),
        description: description.to_owned(),
        preparation_duration_minutes: input.preparation_duration_minutes,
        question_set_count: input.question_set_count,
    };
    let mut tx = interviewer_transaction(pool, interviewer).await?;
    let created = job_repository::insert(&mut tx, &new_job).await?;
    if created {
        audit(
            &mut tx,
            interviewer,
            Some(interviewer),
            new_job.id,
            "job.created",
            json!({"version":1,"job_code":new_job.job_code}),
        )
        .await?;
    }
    let job = job_repository::find_by_request_key(&mut tx, interviewer, input.request_key)
        .await?
        .ok_or(conflict("A job with this Job ID already exists"))?;
    if job.interviewer_id != interviewer
        || job.request_key != input.request_key
        || job.job_code != new_job.job_code
        || job.title != new_job.title
        || job.description != new_job.description
        || job.question_set_count != new_job.question_set_count
        || job.preparation_duration_minutes != new_job.preparation_duration_minutes
    {
        return Err(conflict(
            "Request key was already used with different job details",
        ));
    }
    tx.commit().await?;
    Ok(CreatedJob {
        job,
        created,
    })
}

pub(crate) async fn list(
    pool: &PgPool,
    interviewer: Uuid,
    cursor: Option<Uuid>,
    limit: i64,
    filters: JobFilters,
) -> Result<JobPage> {
    if !(1..=100).contains(&limit) {
        return Err(bad("Limit must be between 1 and 100"));
    }
    if filters.search.chars().count() > 160 || filters.search.contains('\0') {
        return Err(bad("Job search must be at most 160 characters"));
    }
    if !matches!(filters.invitations.as_str(), "all" | "with" | "without")
        || !matches!(filters.sort.as_str(), "newest" | "oldest")
    {
        return Err(bad("Invalid job filter or sort order"));
    }
    let mut tx = interviewer_transaction(pool, interviewer).await?;
    let total = job_repository::matching_count(&mut tx, interviewer, &filters).await?;
    let mut items = job_repository::list(&mut tx, interviewer, cursor, limit + 1, &filters).await?;
    tx.commit().await?;
    let more = items.len() > limit as usize;
    items.truncate(limit as usize);
    let next_cursor = if more {
        items.last().map(|item| item.job.id)
    } else {
        None
    };
    Ok(JobPage {
        total,
        items,
        next_cursor,
    })
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

pub(crate) async fn delete(pool: &PgPool, owner: Uuid, job: Uuid) -> Result<()> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let sessions = job_repository::delete(&mut tx, owner, job).await?;
    tx.commit().await?;

    for session in sessions {
        super::media_service::remove_session_media(owner, session).await;
    }

    Ok(())
}
