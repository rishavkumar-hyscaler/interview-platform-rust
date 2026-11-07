use crate::{
    Result, authenticate,
    dto::audit::{AuditEventResponse, AuditPageResponse},
    pagination::Page,
    services::audit_service,
};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use sqlx::PgPool;
pub(crate) async fn audit_log(
    State(pool): State<PgPool>,
    headers: HeaderMap,
    Query(page): Query<Page>,
) -> Result<Json<AuditPageResponse>> {
    let limit = page.limit()?;
    let (owner, _) = authenticate(&pool, &headers).await?;
    let result = audit_service::list(&pool, owner, page.entity_id, page.cursor, limit).await?;
    Ok(Json(AuditPageResponse {
        items: result
            .items
            .into_iter()
            .map(|e| AuditEventResponse {
                id: e.id,
                interviewer_id: e.interviewer_id,
                actor_id: e.actor_id,
                entity_id: e.entity_id,
                action: e.action,
                metadata: e.metadata,
                created_at: e.created_at,
            })
            .collect(),
        next_cursor: result.next_cursor,
    }))
}
