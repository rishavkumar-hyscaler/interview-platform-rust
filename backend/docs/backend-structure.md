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

## Verification

The HTTP extraction compiled application/test targets and passed eleven focused
existing tests for authentication, transitions, observations, media, integrity weights
and adaptive fallback.

The background extraction compiled application/test targets and passed eight focused
existing tests for generation planning/panic recovery, scoring checkpoints, evidence
chunking and stopped-worker shutdown. A source comparison verified all 72 static
query calls retained SQL text, bind counts and execution modes, plus both dynamic
retrieval queries and their session/set boundary. That comparison is a structural
check, not a live PostgreSQL integration test.

No full test suite, live provider requests, database changes or server restarts.
