//! Durable leases, per-batch checkpoints and three retries independent of scoring.
use super::documents::{self, Chunk};
use crate::{
    generation::{provider::AiProvider, types::vector_text},
    infrastructure::interviewer_transaction,
};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

pub(super) const MAX_ATTEMPTS: i32 = 4;
struct Claim {
    session: Uuid,
    owner: Uuid,
    token: Uuid,
    revision: i64,
}
async fn claim(pool: &PgPool, provider: &AiProvider) -> anyhow::Result<Option<Claim>> {
    let owners: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM users WHERE role='interviewer' ORDER BY id")
            .fetch_all(pool)
            .await?;
    for owner in owners {
        let mut tx = interviewer_transaction(pool, owner).await?;
        // Backfill interviews scored before indexing was introduced; never regrade them.
        sqlx::query("INSERT INTO embedding_jobs(session_id,interviewer_id) SELECT s.id,s.interviewer_id FROM interview_sessions s WHERE EXISTS(SELECT 1 FROM audit_events a WHERE a.entity_id=s.id AND a.action='assessment.completed') AND NOT EXISTS(SELECT 1 FROM embedding_jobs j WHERE j.session_id=s.id) ON CONFLICT(session_id) DO NOTHING").execute(&mut *tx).await?;
        sqlx::query("UPDATE embedding_jobs SET status='pending',revision=revision+1,attempts=0,last_error=NULL,available_at=now(),updated_at=now() WHERE status IN ('completed','failed') AND (embedding_model IS DISTINCT FROM $1 OR embedding_dimensions IS DISTINCT FROM $2)").bind(&provider.embedding_model).bind(provider.dimensions as i32).execute(&mut *tx).await?;
        sqlx::query("UPDATE embedding_jobs SET status='failed',claim_token=NULL,lease_until=NULL,last_error='Embedding worker stopped during its final attempt',updated_at=now() WHERE status='running' AND lease_until<now() AND attempts>=4").execute(&mut *tx).await?;
        let token = Uuid::new_v4();
        let row:Option<(Uuid,i64)>=sqlx::query_as("UPDATE embedding_jobs SET status='running',attempts=attempts+1,claim_token=$1,lease_until=now()+interval '5 minutes',embedding_model=$2,embedding_dimensions=$3,updated_at=now() WHERE session_id=(SELECT session_id FROM embedding_jobs WHERE ((status='pending' AND available_at<=now()) OR (status='running' AND lease_until<now())) AND attempts<4 ORDER BY available_at,session_id FOR UPDATE SKIP LOCKED LIMIT 1) RETURNING session_id,revision")
            .bind(token).bind(&provider.embedding_model).bind(provider.dimensions as i32).fetch_optional(&mut *tx).await?;
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
        sqlx::query("UPDATE embedding_jobs SET status=CASE WHEN revision<>$3 THEN 'pending' WHEN attempts>=4 THEN 'failed' ELSE 'pending' END,attempts=CASE WHEN revision<>$3 THEN 0 ELSE attempts END,last_error=CASE WHEN revision<>$3 THEN NULL ELSE 'Embedding request or index write failed; retry available' END,available_at=CASE WHEN revision<>$3 THEN greatest(available_at,now()+interval '2 seconds') ELSE now()+attempts*interval '5 seconds' END,claim_token=NULL,lease_until=NULL,updated_at=now() WHERE session_id=$1 AND claim_token=$2 AND status='running'")
            .bind(claim.session).bind(claim.token).bind(claim.revision).execute(&mut *tx).await?;
        tx.commit().await?;
    }
    task.finish(success);
    Ok(true)
}
async fn check_claim(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    claim: &Claim,
) -> anyhow::Result<()> {
    let revision:Option<i64>=sqlx::query_scalar("SELECT revision FROM embedding_jobs WHERE session_id=$1 AND claim_token=$2 AND status='running' AND lease_until>now() FOR UPDATE")
        .bind(claim.session).bind(claim.token).fetch_optional(&mut **tx).await?;
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
    sqlx::query("DELETE FROM evidence_embeddings WHERE (session_id=$1 OR question_id=ANY($2)) AND embedding_model=$3 AND embedding_dimensions=$4 AND NOT(source_id=ANY($5))")
        .bind(claim.session).bind(&questions).bind(&provider.embedding_model).bind(provider.dimensions as i32).bind(&ids).execute(&mut *tx).await?;
    sqlx::query("UPDATE embedding_jobs SET status='completed',indexed_revision=revision,last_error=NULL,claim_token=NULL,lease_until=NULL,updated_at=now() WHERE session_id=$1 AND claim_token=$2")
        .bind(claim.session).bind(claim.token).execute(&mut *tx).await?;
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
    let rows:Vec<(String,String)>=sqlx::query_as("SELECT content_hash,embedding::text FROM evidence_embeddings WHERE content_hash=ANY($1) AND embedding_model=$2 AND embedding_dimensions=$3")
        .bind(&hashes).bind(&provider.embedding_model).bind(provider.dimensions as i32).fetch_all(&mut *tx).await?;
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
        sqlx::query("INSERT INTO evidence_embeddings(interviewer_id,session_id,question_id,source_id,source_type,content,content_hash,embedding,embedding_model,embedding_dimensions) VALUES($1,$2,$3,$4,$5,$6,$7,$8::vector,$9,$10) ON CONFLICT(interviewer_id,scope_id,source_id,embedding_model,embedding_dimensions) DO UPDATE SET content=excluded.content,content_hash=excluded.content_hash,embedding=excluded.embedding,updated_at=now() WHERE evidence_embeddings.content_hash<>excluded.content_hash")
            .bind(claim.owner).bind(chunk.session).bind(chunk.question).bind(&chunk.source_id).bind(chunk.kind).bind(&chunk.text).bind(&chunk.hash).bind(vector).bind(&provider.embedding_model).bind(provider.dimensions as i32).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE embedding_jobs SET lease_until=now()+interval '5 minutes',updated_at=now() WHERE session_id=$1 AND claim_token=$2")
        .bind(claim.session).bind(claim.token).execute(&mut *tx).await?;
    tx.commit().await?;
    tracing::info!(session_id=%claim.session,chunks=chunks.len(),generated=missing.len(),"Evidence vector checkpoint saved");
    Ok(())
}
