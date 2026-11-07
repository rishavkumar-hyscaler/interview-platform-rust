# Rig migration: completed AI transport migration

The first migrated feature is JD blueprint extraction. `generation/quality.rs`
calls `AiProvider::blueprint`, which delegates to `ai/structured.rs` using Rig's
OpenAI Responses completion model and a native strict JSON schema.

Rig is pinned to `rig-core = 0.24.0`, compatible with this application's existing
reqwest 0.12 and schemars 1 dependencies. This is an intentional compatibility
pin, not a claim that it is the latest release. Review breaking changes before
upgrading it.

Configuration remains `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL`,
`GENERATION_REQUEST_TIMEOUT_SECONDS` and `GENERATION_DRAFT_REASONING_EFFORT`.
No database migration or frontend change is needed. Already-cached blueprints
remain reusable; the next uncached blueprint request uses Rig.

The existing blueprint prompt, exact slot-count schema, ID canonicalization,
JD evidence validation, competency priorities, two-attempt repair flow, durable
worker retries, cached blueprint and deterministic 2–3 minute time planning stay
in place. Rig does not add another retry layer or expose tools.

`ai/transport.rs` enforces the existing 1 MiB response limit, bounded timeout,
10-second connection timeout and disabled redirects. Provider errors retain the
existing whitelisted diagnostic codes and retry classification. Rig tracing is
disabled for the model future because it can log prompts and responses; the
application logs operation, schema name, engine, duration and request outcome only.

The second stage migrates question drafting, prompt-only generation/review,
reference-answer generation and independent quality/reference review to the same
Rig adapter. `AiProvider::structured` now routes every structured operation
to Rig. It still builds the established schema, output-token cap and
reasoning settings before handing the request to Rig. The transport preserves
schema property authoring order (prompt, answer, then criteria).

The active pipeline remains sequential: draft one complete question, validate its
fixed time budget and rubric, deduplicate it, independently review it, and save
its checkpoint before proceeding. This migration adds no extra model calls and
changes no prompts, worker attempts, content repairs, deduplication thresholds,
checkpoint fingerprints, set assignment, database schema or candidate API.

The third stage migrates `AiProvider::embeddings` to `ai/embeddings.rs`, using
Rig's OpenAI embedding model. This covers question-bank deduplication vectors,
post-scoring evidence indexing and reviewer query embeddings. The separate
`EMBEDDING_API_KEY`, `EMBEDDING_MODEL`, configured dimensions and explicit float
encoding are preserved. Requests use a 90-second timeout and no transport retries.
Empty batches return immediately; invalid dimensions or batches above Rig's
1024-document limit are rejected before sending.

Rig 0.24 discards embedding indices when pairing results with input documents.
The shared transport first validates count, unique in-range indices and response
order, then sorts by index. The adapter validates dimensions and converts to the
existing f32 vectors, rejecting non-finite, overflowed and zero vectors. Existing
1 MiB response bounds and whitelisted provider errors remain in effect.

No vector storage or retrieval changes are needed: existing pgvector tables,
model/dimension checks, cache hashes, 32-document checkpoint batches, similarity
thresholds, RLS and session-scoped SQL remain authoritative. Existing vectors
stay usable without re-embedding solely because of this migration.

The fourth stage routes `rubric_grade` through Rig. Final worker scoring and
adaptive navigation both use the same `scoring/rubric.rs::evaluate` function, so
both now use Rig without changing their prompts or result shape. Scoring keeps
its existing 90-second request timeout independently of the generation timeout;
adaptive navigation still cancels grading after six seconds and falls back to
sequential navigation. No extra reasoning parameter is added to scoring.

Rubric criterion coverage, unique IDs, point limits, exact evidence quotes,
blank-answer zeroes and media-only human review remain deterministic application
checks. Input hashes, validated checkpoint reuse, durable worker retry budgets,
manual retry, revision checks and competency aggregation remain unchanged.
Post-scoring embedding scheduling also remains unchanged.

The final stage migrates reviewer response streaming to `ai/streaming.rs` using
Rig's completion stream. All structured operations now use Rig; the legacy
Responses HTTP client and handwritten provider SSE loop have been removed.
The remaining reqwest transport is an implementation of Rig's HTTP interface,
used to enforce application bounds and error handling rather than bypass Rig.

A raw SSE guard is necessary for Rig 0.24: it otherwise treats refusals as text
and can emit a synthetic final result without a successful provider completion.
The guard requires a real completed event, rejects refusal/failure/incomplete
and tool-call events, and enforces 2 MiB wire / 1 MiB output bounds. Reviewer
requests have a 90-second overall deadline. Streams are consumed once; errors
and disconnected callbacks cancel the request without an automatic resend.
Rig tracing is disabled during stream creation and consumption to avoid logging
session content. Application timing metadata remains available.

Reviewer source retrieval, exact citation checks, session history scope,
unverified-delta labels and verified final persistence remain unchanged.
Integrity questions use this same migrated reviewer stream. The standalone
integrity summary in `review/integrity.rs` is a deterministic count/weight
summary, not a separate AI call; it needs no provider migration.

Each request owns its text accumulator, callback and transport guard. No shared
conversation state has been introduced. Client/model handles are still built
per request; app-wide client reuse and the controller/service/repository
restructure are separate follow-up work.

Scoring still publishes per-question completion events; this migration does
not add token-by-token scoring-rationale streaming.

Focused local verification:

```sh
cargo test --locked --lib ai::structured::tests
cargo test --locked --lib ai::embeddings::tests
cargo test --locked --lib scoring::rig_tests
```

Tests use a loopback mock and dummy credentials, not a live AI provider or the
application database. They check strict schema/count, privacy settings, domain
validation, reasoning output handling, safe error classification, absence of
transport retries, malformed/incomplete output and oversized responses.

The additional question-generation mocks check all migrated operation names,
token/reasoning settings, exact slot counts, coding/null constraints, the fixed
question time, schema property order, safe rate-limit errors and refusals. These
checks do not replace a live-provider acceptance run; no paid AI test was run.

Embedding mocks check reversed response ordering, single-query and 32-document
batches, separate embedding credentials, model/dimensions, float encoding,
malformed vectors/indices/counts, quota and transient failures, and no network
request for empty or invalid batches. Generation/reviewer/RAG mock response
fixtures now include the normal OpenAI embedding metadata required by Rig.

Scoring mocks exercise the real grading function with valid and invalid grades,
quote and criterion validation, saved-grade validation, blank/media-only bypass,
provider retry classification, privacy settings and cancellation. Existing
adaptive fallback and checkpoint tests remain applicable. No live-provider or
application-database acceptance run was performed for this phase.

Final-stage focused checks (local mocks only):

```sh
cargo test --locked --lib ai::streaming::tests
cargo test --locked --lib ai::transport::tests
cargo check --locked --test reviewer
```

Streaming mocks cover separate concurrent request inputs/deltas, strict schema
and privacy options, successful completion, truncated/failed/incomplete/refused
responses, disallowed tool calls, HTTP failure without retransmission, and
reviewer disconnection. Guard checks cover fragmented CRLF frames, malformed
JSON, wire limits and output limits. No database or live-provider tests are
needed for these transport checks.
