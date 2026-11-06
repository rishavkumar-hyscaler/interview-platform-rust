use super::documents;
use crate::{
    generation::{provider::AiProvider, types::vector_text},
    infrastructure::interviewer_transaction,
};
use sqlx::PgPool;
use uuid::Uuid;

pub(crate) struct Retrieved {
    pub sources: Vec<(String, String)>,
    pub total: i64,
}
// Scope is enforced both by RLS and by membership in this session's assigned set.
const ELIGIBLE: &str = "SELECT e.source_id,e.content,e.embedding FROM evidence_embeddings e WHERE e.embedding_model=$2 AND e.embedding_dimensions=$3 AND (e.session_id=$1 OR (e.session_id IS NULL AND EXISTS(SELECT 1 FROM session_question_details() q WHERE q.session_id=$1 AND q.id=e.question_id)))";
pub(crate) async fn search(
    pool: &PgPool,
    owner: Uuid,
    session: Uuid,
    provider: &AiProvider,
    question: &str,
    page: u32,
) -> anyhow::Result<Option<Retrieved>> {
    let mut tx = interviewer_transaction(pool, owner).await?;
    let ready:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM embedding_jobs WHERE session_id=$1 AND status='completed' AND revision=indexed_revision AND embedding_model=$2 AND embedding_dimensions=$3)")
        .bind(session).bind(&provider.embedding_model).bind(provider.dimensions as i32).fetch_one(&mut *tx).await?;
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
    let current:Option<Uuid>=sqlx::query_scalar("SELECT session_id FROM embedding_jobs WHERE session_id=$1 AND status='completed' AND revision=indexed_revision AND embedding_model=$2 AND embedding_dimensions=$3 FOR SHARE")
        .bind(session).bind(&provider.embedding_model).bind(provider.dimensions as i32).fetch_optional(&mut *tx).await?;
    if current.is_none() {
        tx.commit().await?;
        return Ok(None);
    }
    let sql = format!(
        "WITH eligible AS MATERIALIZED ({ELIGIBLE}) SELECT source_id,content FROM eligible ORDER BY embedding <=> $4::vector,source_id OFFSET $5 LIMIT 6"
    );
    let sources = sqlx::query_as(&sql)
        .bind(session)
        .bind(&provider.embedding_model)
        .bind(provider.dimensions as i32)
        .bind(query)
        .bind(i64::from(page) * 6)
        .fetch_all(&mut *tx)
        .await?;
    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM ({ELIGIBLE}) scoped"))
        .bind(session)
        .bind(&provider.embedding_model)
        .bind(provider.dimensions as i32)
        .fetch_one(&mut *tx)
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
