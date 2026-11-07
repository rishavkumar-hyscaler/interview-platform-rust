# AI workflows and Rig

## Provider integration

All structured generation and scoring, reviewer streaming, and embeddings use
`rig-core = 0.24.0` through `src/ai/structured.rs`, `streaming.rs` and
`embeddings.rs`. `transport.rs` implements Rig's HTTP transport with bounded
responses, timeouts, disabled redirects and safe error classification.
Application validation, SQL retrieval and durable retries remain outside Rig.
There are no Rig tools, shared conversation memory or Rig vector-store adapters.

Rig adapters are centralized; client/model handles are still constructed per
request. An app-wide shared AI client has not been implemented. Each request owns
its input, stream accumulator and callback.

Configuration uses `LLM_BASE_URL`, `LLM_API_KEY`, `LLM_MODEL`,
`EMBEDDING_API_KEY`, `EMBEDDING_MODEL` and the timeout/reasoning settings in
`backend/.env.example`. Never commit real credentials.

Transport limits are 1 MiB for normal responses/output and 2 MiB for streamed
wire data. Structured output uses strict schemas. The streaming guard requires
an actual successful provider completion and rejects refusals, failures,
incomplete responses and tool calls. Embeddings validate count, indices,
dimensions and finite nonzero vectors before persistence. Provider payload
tracing is suppressed; application logs retain operational metadata only.

## Reusable sets and assignment


Creating a job atomically queues the interviewer-selected 1–5 question sets (default 1). The interviewer chooses a preparation duration of 5–180 minutes. Each set uses the job blueprint and sequential question/reference/rubric generation with validation, embedding deduplication and independent review. New questions are scoped to 2–3 minutes, and their count is planned to fit the duration.

The Question sets panel shows status, saved progress, reference answers, time plans and bounded failure details. Generation runs only for job sets. There is no per-session generation fallback or automatic replacement on assignment.

Invitations require a completed, nonempty set matching the duration. The transaction links `interview_sessions.question_set_id` to a reusable set, balancing usage across configured sets. Every question belongs to `interview_questions.set_id`; questions are never copied per candidate. Answers and progress remain scoped by session and question. Prepared sets remain usable with the AI provider offline.

Generation state, initial attempt plus three retries, next-run time and worker leases live only in `question_sets`. Retries preserve saved question checkpoints. After four failed worker attempts, an interviewer can explicitly retry the job's failed sets.

## Storage and API

The single development baseline is `202611040001_initial_schema.sql`. Question text, worked references, rubrics and embeddings are stored in `interview_questions`; candidate answers are in `interview_answers`. The SECURITY INVOKER `session_question_details()` function projects questions through session assignment and preserves row-level security.

- `POST /api/v1/jobs`: accepts `question_set_count` and `preparation_duration_minutes` alongside the job fields.
- `GET /api/v1/jobs/{id}/question-sets`: interviewer-only set status, questions and references.
- `POST /api/v1/jobs/{id}/question-sets`: explicit retry of failed configured sets.
- `GET /api/v1/sessions/{id}/generation`: inspect the assigned set and that candidate's answers; this endpoint does not schedule generation.

No candidate API returns reference answers or rubrics.

## Generation policy

## Automatic retry policy

Every generation failure now receives up to three automatic worker retries after the initial attempt (four total attempts), including validation rejection, duplicates, provider errors, unexpected response formats, and unexpected preparation panics. Retry delays are 5, 10 and 15 seconds. Task attempts and next-run times are stored in PostgreSQL, so restarting the backend does not reset the budget. A failed preparation task cannot terminate the worker loop; cancellation also aborts its in-flight work.

Saved checkpoints and the time plan are reused. The next worker attempt receives persisted review findings and the previous rejected draft. Existing bounded local draft/repair calls remain inside each worker attempt, so four worker attempts does not mean only four provider requests. After the fourth failure, the set requires an explicit interviewer retry. Existing failed records are not silently restarted by this code change. No live-provider tests were run.

## Time planning

Before drafting any question, the backend plans all remaining slots and persists the plan as a `generation.planned` audit event for the set. New questions get fixed allocations of 2–3 minutes. A 10-minute fresh interview uses at most five 2-minute questions; a 30-minute interview with six questions uses up to eighteen task minutes, leaving review time. Priority for a third minute combines answer type and difficulty. Code scope is one small function or a focused fix in supplied starter code (normally 3–8 changed lines); scenario/text scope is one decision/concept answered in 2–4 sentences. Full components, services and multi-feature workflows are excluded.

The blueprint request count is capped by duration / 2. Older cached blueprints are adapted locally to fit, keeping higher-priority competencies. Existing saved questions and their times are preserved. A legacy partial set may therefore retain some earlier one-minute questions; only new questions follow the 2–3 minute policy. A set-specific plan is reused across retries, so cached job blueprints and saved questions do not need paid regeneration. The strict output schema fixes estimated_minutes to the planned allocation. Quality review checks scope and correctness independently. A time plan does not guarantee that every person will finish in exactly that time.

The interviewer can expand Question time plan to see the number of questions, each allocation and scope, and the remaining review time. No new database table or migration is required.

## Per-question generation and API usage

For each unsaved position, generate the complete question, reference answer and rubric in one call. Validate its structure, time allocation and exact fingerprint in Rust before requesting an embedding. Check semantic similarity against the job bank, then use one independent content review for both prompt and reference. Save only the fully validated question before proceeding.

A successful six-question set uses 6 draft calls + 6 embedding calls + 6 review calls, plus one blueprint call when the job has no cached blueprint. Each question allows one duplicate replacement and one content repair at most (three drafts maximum); unchanged prompts reuse their embedding during content repair; it does not regenerate previous checkpoints. Invalid local drafts and duplicates skip review. Transient service errors can still trigger the existing bounded task retries, so total usage can be higher on failures. No live AI test was run for this change.

The combined draft may spend answer tokens on a question that is subsequently rejected as a duplicate. This trades some wasted tokens on duplicate drafts for fewer requests on successful questions.


## Scoring and evidence indexing

Scoring validates rubric criterion coverage, point bounds and exact answer quotes.
Blank answers receive zero; media-only responses require human review. Saved
checkpoints are keyed by answer, rubric and model inputs. Submission queues durable
scoring; completed scoring queues evidence indexing. Adaptive navigation bounds
live grading to six seconds and falls back to sequential navigation.

Reviewer streaming retrieves only the current session's evidence, validates final
citations and persists verified final replies. Deltas remain unverified until
completion. Integrity summaries are deterministic counts/weights. See [RAG](rag.md)
for chunk storage and retrieval and [API](api.md) for status/manual retry routes.

## Verification limits

Local/mock tests cover transport bounds, malformed responses, refusal handling,
embedding ordering, scoring validation and concurrent stream isolation. Historical
live generation trials encountered quality rejections; those tests do not establish
full provider acceptance or a generation speed guarantee. Scoring rationale is not
streamed token by token. No live provider calls are needed for documentation edits.
