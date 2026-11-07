//! HTTP extraction and explicit DTO mapping; no SQL or generation rules.
use crate::{
    Result, authenticate,
    dto::job::{CreateJobRequest, JobListItemResponse, JobListResponse, JobResponse},
    models::job::Job,
    pagination::Page,
    services::job_service::{self, CreateJob},
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use sqlx::PgPool;

pub(crate) async fn create_job(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Json(input): Json<CreateJobRequest>,
) -> Result<(StatusCode, Json<JobResponse>)> {
    let (interviewer, _) = authenticate(&pool, &headers).await?;
    let result = job_service::create(
        &pool,
        interviewer,
        CreateJob {
            job_code: input.job_code,
            title: input.title,
            description: input.description,
            request_key: input.request_key,
            preparation_duration_minutes: input.preparation_duration_minutes,
            question_set_count: input.question_set_count,
        },
    )
    .await?;
    let status = if result.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(response(result.job))))
}

pub(crate) async fn list_jobs(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<JobListResponse>> {
    let limit = page.limit()?;
    let (interviewer, _) = authenticate(&pool, &headers).await?;
    let result = job_service::list(&pool, interviewer, page.cursor, limit).await?;
    Ok(Json(JobListResponse {
        items: result
            .items
            .into_iter()
            .map(|item| JobListItemResponse {
                job: response(item.job),
                session_count: item.session_count,
            })
            .collect(),
        next_cursor: result.next_cursor,
    }))
}

fn response(job: Job) -> JobResponse {
    JobResponse {
        id: job.id,
        job_code: job.job_code,
        title: job.title,
        description: job.description,
        created_at: job.created_at,
        preparation_duration_minutes: job.preparation_duration_minutes,
        question_set_count: job.question_set_count,
    }
}
