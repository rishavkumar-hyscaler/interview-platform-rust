# Persistent evidence retrieval

The flow is `interview submitted → scoring completed → embedding job → evidence search ready`. Scoring and indexing are separate outcomes. Existing completed assessments are queued automatically when the embedding worker starts; they are not scored again.

`embedding_jobs` stores one job per session, revision/indexed revision, status, attempts, available time and a fenced five-minute lease. `evidence_embeddings` stores source IDs/types, original readable chunk text, SHA-256 content hash, pgvector vector, model and dimensions. Answers, assessment results and events are session scoped. Prompt/rubric chunks are question scoped and reused by all candidates assigned that question set. Question-generation deduplication vectors remain separate because those describe prompts rather than answer/rubric evidence.

Database triggers queue the job atomically when a completed assessment is inserted. Late proctoring events, attachment metadata, reviewer annotations, answer-content changes and shared question/rubric updates invalidate the affected index. New revisions reset their retry budget. Raw media files are not embedded or automatically transcribed; event text links them to their audit evidence IDs.

The worker chunks text into up to 1,800 Unicode characters, overlapping by 200. It checks cached hashes for the same owner/model/dimensions and embeds only missing/changed text, in batches of at most 32. Each saved batch is a checkpoint. Duplicate text in a batch needs one embedding. Repeated rubric/prompt chunks are reused across interviews without duplicating their stored vectors.

A run gets one attempt plus three automatic retries, with 5/10/15-second backoff. Provider failures, validation failures and persistence errors do not change `interview_sessions.state` or assessment results. An interrupted worker is reclaimed after its lease expires; earlier saved batches are reused. Publication checks the current revision and claim token; a concurrent source change schedules fresh work rather than marking stale content ready. An exhausted job remains failed until explicitly retried or new evidence changes its revision. Model/dimension changes queue a new index; mismatched vectors are never queried together.

Chat checks readiness, embeds only the interviewer’s question and searches the entire eligible index before pagination. Each page has six ranked chunks, leaving room for exact SQL integrity counts and the stored competency summary. Source IDs remain attached to text and all final model quotes are checked against those sources. Keyword fallback loads this session’s current raw evidence and does not regenerate embeddings. Best/weakest competency comparisons and integrity counts avoid embedding calls. The UI displays index status and a retry control separately from chat.

RLS enforces interviewer ownership. Retrieval additionally accepts only this session’s answer/event/assessment chunks plus question chunks belonging to its assigned set. Another candidate’s answers are excluded even when both candidates share a question set. pgvector uses exact cosine search over the scoped subset; there is no approximate global index or cross-session top-k filtering. Session/question deletion cascades to its dependent index rows.

## Development schema

There remains one migration: `202611040001_initial_schema.sql`. For the immediately preceding development baseline, run `cargo run -- --upgrade-rag` from `backend/`. It verifies the old checksum, adds the RAG schema in one transaction and updates that baseline checksum. It refuses unexpected migration histories. Fresh databases use `cargo run -- --migrate`. No existing application rows are deleted. This development rebaseline is not a production migration strategy.

## Focused verification

`cargo test --lib rag::documents` checks stable Unicode chunking and readable quoting. The ignored `rag::tests::durable_embeddings_reuse_checkpoints_recover_and_remain_session_scoped` test needs a disposable migrated database in `RAG_TEST_DATABASE_URL`, using the restricted application role. It starts its own local embedding mock and never loads provider secrets or calls a live AI service. It checks post-scoring queueing, saved-vector reuse, query-only embeddings, partial-batch recovery, stale leases, source updates, shared rubrics, pagination, owner/candidate isolation and exhausted retries preserving scores. Remove that disposable database after the run.
