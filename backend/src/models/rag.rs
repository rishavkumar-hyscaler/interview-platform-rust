use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(crate) struct IndexJob {
    pub status: String,
    pub attempts: i32,
    pub revision: i64,
    pub indexed_revision: i64,
    pub last_error: Option<String>,
    pub model: Option<String>,
    pub dimensions: Option<i32>,
}
pub(crate) struct IndexStatus {
    pub job: Option<IndexJob>,
    pub scored: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct EvidenceChunk {
    pub source_id: String,
    pub kind: &'static str,
    pub text: String,
    pub hash: String,
    pub session: Option<Uuid>,
    pub question: Option<Uuid>,
}

pub(crate) struct Retrieved {
    pub sources: Vec<(String, String)>,
    pub total: i64,
}

/// Database insert values; separate from API input and AI generation schemas.
pub(crate) struct EvidenceEmbeddingInsert<'a> {
    pub owner: Uuid,
    pub session: Option<Uuid>,
    pub question: Option<Uuid>,
    pub source_id: &'a str,
    pub kind: &'a str,
    pub content: &'a str,
    pub hash: &'a str,
    pub vector: &'a str,
    pub model: &'a str,
    pub dimensions: i32,
}
