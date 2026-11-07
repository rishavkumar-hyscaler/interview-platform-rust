//! Private, owner-scoped rejection details; never written to request/provider logs.
use super::quality;
use crate::infrastructure::interviewer_transaction;
use crate::repositories::generation_workflow_repository as repo;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub(super) struct Scope {
    pub pool: PgPool,
    pub owner: Uuid,
    pub entity: Uuid,
    pub position: Option<usize>,
}
impl Scope {
    pub async fn rejected(
        &self,
        phase: &str,
        attempt: usize,
        error: &anyhow::Error,
        draft: &Option<Value>,
    ) {
        let result = async {
            let mut tx = interviewer_transaction(&self.pool,self.owner).await?;
            let metadata = json!({"actor":"worker","phase":phase,"attempt":attempt,"question_position":self.position,"summary":error.to_string().chars().take(1000).collect::<String>(),"findings":quality::review_findings(error),"draft":draft.as_ref().filter(|draft|draft.to_string().len()<=80000)});
            repo::record_rejection(&mut tx,Uuid::new_v4(),self.owner,self.entity,metadata).await?;
            tx.commit().await
        }.await;
        if result.is_err() {
            tracing::warn!(
                event = "generation.diagnostics_unavailable",
                "Could not persist private rejection details"
            );
        }
    }
}
