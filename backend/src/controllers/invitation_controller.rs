use crate::{
    Result, authenticate,
    dto::invitation::{InvitationRequest, InvitationResponse, RecoveredInvitationResponse},
    services::invitation_service::{self, InviteCandidate},
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use sqlx::PgPool;
use uuid::Uuid;
pub(crate) async fn invite(
    State(pool): State<PgPool>,
    Path(job): Path<Uuid>,
    headers: HeaderMap,
    Json(input): Json<InvitationRequest>,
) -> Result<(StatusCode, Json<InvitationResponse>)> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    let result = invitation_service::invite(
        &pool,
        owner,
        job,
        InviteCandidate {
            candidate_name: input.candidate_name,
            candidate_email: input.candidate_email,
            duration_minutes: input.duration_minutes,
            request_key: input.request_key,
        },
    )
    .await?;
    Ok((
        if result.created && result.token.is_none() {
            StatusCode::ACCEPTED
        } else if result.created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(InvitationResponse {
            session: super::interview_controller::response(result.session),
            invitation_token: result.token,
        }),
    ))
}
pub(crate) async fn retrieve_invitation(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<RecoveredInvitationResponse>> {
    let (owner, _) = authenticate(&pool, &headers).await?;
    let result = invitation_service::retrieve_invitation(&pool, owner, id).await?;
    Ok(Json(RecoveredInvitationResponse {
        invitation_token: result.token,
        expires_at: result.expires_at,
        replaced_legacy_link: result.replaced_legacy_link,
    }))
}
