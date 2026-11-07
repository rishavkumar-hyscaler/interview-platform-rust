use super::documents;
use crate::repositories::evidence_index_repository as repo;
use crate::{
    generation::{provider::AiProvider, types::vector_text},
    infrastructure::interviewer_transaction,
};
use sqlx::PgPool;
use uuid::Uuid;

pub(crate) use crate::models::rag::Retrieved;
// Scope is enforced both by RLS and by membership in this session's assigned set.
pub(crate) async fn search(
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
    provider: &AiProvider,
    question: &str,
    page: u32,
) -> anyhow::Result<Option<Retrieved>> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let ready: bool = repo::ready(
        &mut tx,
        session,
        &provider.embedding_model,
        provider.dimensions as i32,
    )
    .await?;
    tx.commit().await?;
    if !ready {
        return Ok(None);
    }
    // Exactly one text is embedded per chat request. Stored evidence is never re-embedded here.
    let vectors = match provider.embeddings(&[question.to_string()]).await {
        Ok(vectors) => vectors,
        Err(_) => {
            tracing::warn!(%session,"Query embedding failed; using session keyword retrieval");
            return Ok(None);
        }
    };
    let query = vector_text(&vectors[0])?;
    let mut tx = interviewer_transaction(pool, owner).await?;
    // A late event can dirty the index while the query embedding is in flight.
    let current: Option<Uuid> = repo::lock_current_index(
        &mut tx,
        session,
        &provider.embedding_model,
        provider.dimensions as i32,
    )
    .await?;
    if current.is_none() {
        tx.commit().await?;
        return Ok(None);
    }
    let sources = repo::search(
        &mut tx,
        session,
        &provider.embedding_model,
        provider.dimensions as i32,
        query,
        i64::from(page) * 6,
    )
    .await?;
    let total: i64 = repo::source_count(
        &mut tx,
        session,
        &provider.embedding_model,
        provider.dimensions as i32,
    )
    .await?;
    tx.commit().await?;
    Ok(Some(Retrieved { sources, total }))
}
pub(crate) async fn fallback(
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
    question: &str,
    page: u32,
) -> anyhow::Result<Retrieved> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let chunks = documents::load(&mut tx, session).await?;
    tx.commit().await?;
    let terms: Vec<_> = question
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() > 2)
        .map(String::from)
        .collect();
    let mut ranked: Vec<_> = chunks
        .into_iter()
        .map(|chunk| {
            let text = chunk.text.to_lowercase();
            let score = terms
                .iter()
                .filter(|term| text.contains(term.as_str()))
                .count();
            (score, chunk)
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.source_id.cmp(&b.1.source_id)));
    let total = ranked.len() as i64;
    let sources = ranked
        .into_iter()
        .skip(page as usize * 6)
        .take(6)
        .map(|(_, c)| (c.source_id, c.text))
        .collect();
    Ok(Retrieved { sources, total })
}
