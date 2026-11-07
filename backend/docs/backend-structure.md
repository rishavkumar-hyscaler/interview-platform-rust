# Backend layers

HTTP features and background workflows now use separate controllers, services,
repositories, internal models and API DTOs. Services own business decisions and
transactions; repositories execute domain SQL and decode query rows.
No database migration, dependency change or frontend change is needed.

## Directory map

| Directory | Responsibility |
| --- | --- |
| `src/controllers/` | HTTP extraction, authentication context, route composition, response DTO mapping, SSE framing |
| `src/services/` | Business rules, transaction boundaries, validation and workflow orchestration |
| `src/repositories/` | Endpoint and background SQL; caller-owned transactions |
| `src/models/` | Internal database projections and application values |
| `src/dto/` | Explicit API requests and response fields |
| `src/ai/` | Rig structured/streaming/embedding adapters and bounded transport |
| `src/workers/mod.rs` | Background polling loops and shutdown coordination |
| `src/app.rs` | Central router composition, health checks and HTTP-wide policies |
| `src/access.rs` | HTTP credential checks before opening scoped transactions |
| `src/infrastructure.rs` | Environment/tracing setup, connection pools and transaction-local RLS context |

Controllers contain no SQL. Services called by controllers do not accept Axum
extractors, HTTP headers or API DTOs. They receive application commands and either
open an interviewer-scoped transaction or consume the authenticated candidate
transaction supplied by the controller. Services own commits and rollback boundaries.
Repositories do not commit those transactions or depend on DTOs.

## Migrated HTTP flows

- Jobs: create and paginated listing.
- Authentication: registration, login, current user and logout.
- Interviews: summary, session listing/details, invitations and link recovery.
- Candidate room: lifecycle transitions, question delivery and revision-safe saves.
- Adaptive navigation: cached grading, bounded live grading and fallback selection.
- Scoring: assessment inspection and explicit retry.
- Generation: preparation inspection and question-set retry.
- RAG: embedding job status and retry after scoring.
- Proctoring: observation validation, limits and signed receipt-chain persistence.
- Evidence/recordings: uploads, private authorized reads and integrity checks.
- Review: annotations, integrity summary, export, analytics, metrics and SSE chat/live events.
- Audit: explicit event projection and pagination.

Database session rows are no longer serialized wholesale. Interviewer list/detail
and invitation responses use explicitly selected fields and separate response DTOs.
Candidate projections exclude reference answers and rubrics. Password hashes,
sealed invitation tokens and request keys are not response DTO fields. JSON fields
remain for schema-defined AI output and selected evidence metadata; these are not
whole-row serialization. Media bytes and SSE events have dedicated HTTP adapters.

Long-running reviewer and adaptive operations revalidate the session before writing
private context after an AI call. Upload limits, signature/hash validation, immutable
file writes, cleanup after failed inserts, owner scoping, retry rules and transaction
locks are retained.

## Background workflows

`src/services/generation/`, `src/services/scoring/`, `src/services/rag/` and
`src/services/review/` contain the existing domain helpers and background workflows.
Public Rust entry points such as `platform_api::generation`, `platform_api::scoring`,
`platform_api::rag` and `platform_api::worker` remain available through re-exports.
There is still one Cargo package and one running backend process.

Background query ownership:

- `src/repositories/generation_workflow_repository.rs`: set assignment, queue claims,
  lease renewal, blueprint/plan persistence, duplicate checks and question checkpoints.
- `src/repositories/scoring_task_repository.rs`: scoring claims, stale-attempt recovery,
  current claim checks, saved question grades and completion state.
- `src/repositories/evidence_index_repository.rs`: embedding leases, invalidation,
  cached vectors, chunk writes and session-scoped retrieval.
- `src/repositories/expiry_repository.rs`: due-session selection, locking, finalization
  and expiry audit persistence.
- `src/services/expiry_service.rs`: expiry workflow and transaction ownership;
  infrastructure re-exports the previous functions for existing callers.

No production SQL or query execution remains in controllers or services. Database
setup/transaction-local RLS configuration, schema management and the readiness probe
remain infrastructure responsibilities. Test fixtures may execute their own SQL.

Services retain lease checks, checkpoint ordering, repair/retry policy, hashing,
vector validation and transaction commits. Repositories preserve SQL errors so the
existing worker retry classification remains effective. Multi-column generation query
rows and larger insert commands use internal models; no DTO/schema changes are needed.

## Ownership and storage


`users.role` has exactly two values: `interviewer` and `interviewee`. Interviewers sign in with email/password, create jobs and invitations, and review evidence. Reviewing is an interviewer capability, not a separate user role. Interviewees access their assigned session through private invitation links. An invitation creates or reuses an interviewee profile; it does not assign a password. Email is unique within each role.

There are no organizations or memberships. Jobs belong directly to an interviewer. Interview sessions identify both their interviewer and interviewee. Questions belong to reusable job sets; answers belong to a session and question. Forced PostgreSQL row-level security uses transaction-local `app.interviewer_id` derived from authenticated identity. Composite foreign keys prevent cross-interviewer references. The runtime database role cannot bypass RLS.

## Ten application tables

| Table | Responsibility |
|---|---|
| users | Interviewer/interviewee identity |
| auth_sessions | Hashed interviewer login tokens and interviewee invitation capabilities |
| jobs | Job description and competency blueprint |
| question_sets | Reusable job sets and generation retry/lease state |
| interview_sessions | Participants, assigned set, timing and lifecycle |
| embedding_jobs | Independent post-scoring indexing state, revisions, retry budget and leases |
| evidence_embeddings | Owned/session-or-question-scoped text chunks and reusable pgvector embeddings |
| interview_questions | Question snapshots, rubrics, and embedding-backed per-job question bank |
| interview_answers | Revisioned answers |
| audit_events | Append-only action/evidence history |

Authentication and invitation tokens share storage but have distinct authorization paths. Interviewer authentication rejects invitation capabilities. Invitation lookup verifies the interviewee role and restores only that capability's session context. Recoverable invitation secrets use authenticated encryption with a separately configured key. API responses disable caching.

## Operational logging


Backend logs are JSON on the terminal running `cargo run`. Default level is `info`; use `RUST_LOG=platform_api=debug,tower_http=warn,sqlx=warn cargo run` for domain-event diagnostics.

Frontend logs appear in browser DevTools → Console. Enable Debug/Verbose messages to see successful requests and tasks. Development defaults to `debug`; production defaults to `warn`. Set `VITE_LOG_LEVEL=debug`, `info`, `warn`, `error` or `silent` in `frontend/.env.local` and restart Vite to override it. Backend uses `RUST_LOG`; these settings are independent. Logs are not stored in additional tables or sent to a logging service.

| Event | Useful fields |
| --- | --- |
| `request.started` | Request ID, method, route template |
| Backend `request.completed` / frontend `request.response` | Same request ID, status, duration in milliseconds |
| Frontend `request.failed` | Request ID, network error or cancellation, duration |
| `task.started`, `task.completed` | Operation, task ID, outcome, duration; backend tasks also include session ID |
| Backend `task.interrupted` | Cancelled or failed before explicit completion, including shutdown |
| Backend `task.phase` | Generation phase and draft attempt, inside task span |
| Backend `provider.started`, `provider.completed` | AI operation, success and duration, inside task span when applicable |
| Backend `domain_event.staged` (debug) | Audit action and entity ID; transaction may still roll back |

The browser generates `X-Request-ID`; the API validates it as a UUID, replaces invalid values, and returns it in `X-Request-ID`. JSON actions, review streams and uploads link their frontend task to that ID. Search the same ID in browser and API logs. Request/task IDs are diagnostic labels, never authorization credentials. Deferred workers use their own task ID and session ID; use session ID to follow queued work. SSE HTTP duration ends when response headers are ready; its separate task duration covers the stream.

Only route templates and operational identifiers/statuses are logged. Unknown routes become `unmatched`. Passwords, bearer tokens, invitation links, API keys, raw URLs/query strings, prompts, reference answers, candidate answers, media and chat text are excluded. Database failures log the database error code rather than the raw message.
