//! Durable leases, per-batch checkpoints and three retries independent of scoring.
use super::documents::{self, Chunk};
use crate::repositories::evidence_index_repository as repo;
use crate::{
    generation::{provider::AiProvider, types::vector_text},
    infrastructure::interviewer_transaction,
};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

pub(crate) const MAX_ATTEMPTS: i32 = 4;
struct Claim {
    session: Uuid,
    owner: Uuid,
    token: Uuid,
    revision: i64,
}
async fn claim(pool: &PgPool, provider: &AiProvider) -> anyhow::Result<Option<Claim>> {
    let owners: Vec<Uuid> = repo::interviewers(pool).await?;
    for owner in owners {
        let mut tx = interviewer_transaction(pool, owner).await?;
        // Backfill interviews scored before indexing was introduced; never regrade them.
        repo::backfill_jobs(&mut tx).await?;
        repo::invalidate_model(
            &mut tx,
            &provider.embedding_model,
            provider.dimensions as i32,
        )
        .await?;
        repo::expire_final_attempts(&mut tx).await?;
        let token = Uuid::new_v4();
        let row: Option<(Uuid, i64)> = repo::claim(
            &mut tx,
            token,
            &provider.embedding_model,
            provider.dimensions as i32,
        )
        .await?;
        tx.commit().await?;
        if let Some((session, revision)) = row {
            return Ok(Some(Claim {
                session,
                owner,
                token,
                revision,
            }));
        }
    }
    Ok(None)
}
pub async fn process_next(pool: &PgPool, provider: &AiProvider) -> anyhow::Result<bool> {
    let Some(claim) = claim(pool, provider).await? else {
        return Ok(false);
    };
    let mut task = crate::observability::TaskLog::start("rag.index", claim.session);
    let result = index(pool, provider, &claim).await;
    let success = result.is_ok();
    if let Err(error) = result {
        // Diagnostic category only; never log answer text, vectors or provider bodies.
        tracing::warn!(session_id=%claim.session,database_error=error.downcast_ref::<sqlx::Error>().is_some(),"Evidence indexing failed; saved scoring and vector checkpoints are preserved");
        let mut tx = interviewer_transaction(pool, claim.owner).await?;
        repo::fail_attempt(&mut tx, claim.session, claim.token, claim.revision).await?;
        tx.commit().await?;
    }
    task.finish(success);
    Ok(true)
}
async fn check_claim(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    claim: &Claim,
) -> anyhow::Result<()> {
    let revision: Option<i64> = repo::lock_revision(tx, claim.session, claim.token).await?;
    anyhow::ensure!(
        revision == Some(claim.revision),
        "Evidence changed or embedding lease was lost"
    );
    Ok(())
}
async fn index(pool: &PgPool, provider: &AiProvider, claim: &Claim) -> anyhow::Result<()> {
    let mut tx = interviewer_transaction(pool, claim.owner).await?;
    check_claim(&mut tx, claim).await?;
    let chunks = documents::load(&mut tx, claim.session).await?;
    tx.commit().await?;
    anyhow::ensure!(!chunks.is_empty(), "No scored evidence to index");
    for batch in chunks.chunks(32) {
        checkpoint(pool, provider, claim, batch).await?;
    }
    let mut tx = interviewer_transaction(pool, claim.owner).await?;
    check_claim(&mut tx, claim).await?;
    let ids: Vec<_> = chunks.iter().map(|c| c.source_id.clone()).collect();
    let questions: Vec<_> = chunks.iter().filter_map(|c| c.question).collect();
    repo::delete_stale_chunks(
        &mut tx,
        claim.session,
        &questions,
        &provider.embedding_model,
        provider.dimensions as i32,
        &ids,
    )
    .await?;
    repo::complete(&mut tx, claim.session, claim.token).await?;
    crate::audit::record(&mut tx,claim.owner,None,claim.session,"rag.completed",serde_json::json!({"revision":claim.revision,"chunks":chunks.len(),"model":provider.embedding_model,"dimensions":provider.dimensions})).await.map_err(|_|anyhow::anyhow!("Embedding completion audit failed"))?;
    tx.commit().await?;
    tracing::info!(session_id=%claim.session,chunks=chunks.len(),"Session evidence embeddings ready");
    Ok(())
}
async fn checkpoint(
    pool: &PgPool,
    provider: &AiProvider,
    claim: &Claim,
    chunks: &[Chunk],
) -> anyhow::Result<()> {
    let hashes: Vec<_> = chunks.iter().map(|c| c.hash.clone()).collect();
    let mut tx = interviewer_transaction(pool, claim.owner).await?;
    check_claim(&mut tx, claim).await?;
    let rows: Vec<(String, String)> = repo::cached_vectors(
        &mut tx,
        &hashes,
        &provider.embedding_model,
        provider.dimensions as i32,
    )
    .await?;
    tx.commit().await?;
    let mut vectors: HashMap<String, String> = rows.into_iter().collect();
    let mut missing = Vec::new();
    for chunk in chunks {
        if !vectors.contains_key(&chunk.hash)
            && !missing.iter().any(|c: &&Chunk| c.hash == chunk.hash)
        {
            missing.push(chunk);
        }
    }
    if !missing.is_empty() {
        let texts: Vec<_> = missing.iter().map(|c| c.text.clone()).collect();
        let generated = provider.embeddings(&texts).await?;
        anyhow::ensure!(
            generated.len() == missing.len(),
            "Invalid embedding batch size"
        );
        for (chunk, vector) in missing.iter().zip(generated) {
            vectors.insert(chunk.hash.clone(), vector_text(&vector)?);
        }
    }
    let mut tx = interviewer_transaction(pool, claim.owner).await?;
    check_claim(&mut tx, claim).await?;
    for chunk in chunks {
        let vector = vectors
            .get(&chunk.hash)
            .ok_or_else(|| anyhow::anyhow!("Missing generated embedding"))?;
        repo::upsert_chunk(
            &mut tx,
            crate::models::rag::EvidenceEmbeddingInsert {
                owner: claim.owner,
                session: chunk.session,
                question: chunk.question,
                source_id: &chunk.source_id,
                kind: chunk.kind,
                content: &chunk.text,
                hash: &chunk.hash,
                vector: vector,
                model: &provider.embedding_model,
                dimensions: provider.dimensions as i32,
            },
        )
        .await?;
    }
    repo::renew_lease(&mut tx, claim.session, claim.token).await?;
    tx.commit().await?;
    tracing::info!(session_id=%claim.session,chunks=chunks.len(),generated=missing.len(),"Evidence vector checkpoint saved");
    Ok(())
}
