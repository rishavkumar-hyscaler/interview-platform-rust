# AI Interview & Proctoring Platform

Rust and Axum backend, React and TypeScript frontend, PostgreSQL and pgvector. Interviewers create jobs, prepare reusable question sets, invite candidates, and review scores and proctoring evidence. Candidates enter through private invitation links.

## Contents

- [Backend setup](#backend-setup) and [frontend setup](#frontend-setup)
- [Database schema](#schema) and [backend layers](#backend-layers)
- [API contract](#api-contract)
- [AI workflows and Rig](#ai-workflows-and-rig)
- [Question-bank analytics and formulas](#question-bank-analytics)
- [Persistent evidence retrieval](#persistent-evidence-retrieval)
- [Camera and audio proctoring](#cameraaudio-proctoring)
- [Implementation status and limits](#assignment-implementation-status)

One Rust/Axum backend executable serving HTTP and processing durable PostgreSQL background jobs. The backend supports interviewer/interviewee access, direct job ownership, private invitations, AI question preparation, rubric snapshots, timed interviews, revisioned autosave, resumable navigation, submission, and audit history. The React frontend lives in [frontend/](frontend/).

## Backend setup

Requires Rust stable and PostgreSQL with pgvector. Use your existing PostgreSQL at `localhost:5432/interview-platform`. Copy `.env.example` to `.env` only if you do not already have one; preserve your own credentials. The backend automatically loads `.env` when started from `backend/`.

- `DATABASE_URL`: existing connection, used for migrations by default.
- `MIGRATION_DATABASE_URL`: optional migration connection override.
- `RUNTIME_DATABASE_URL`: restricted application connection, used by the backend instead of `DATABASE_URL` when set.

Provision `platform_app` as a non-superuser without BYPASSRLS before applying migrations; the migrations grant its table permissions. Use a private password, not a committed credential. The backend rejects superuser/BYPASSRLS runtime roles. Keep registration enabled only for intentional local/demo onboarding.

```bash
cd backend
cargo run -- --migrate
cargo run
```

The same process starts the expiry job loop automatically; no second backend command is needed. Ctrl-C or SIGTERM starts shutdown of HTTP and background processing. Active connections get at most five seconds to finish; a second signal skips that wait. Database cleanup is capped at one additional second so open live-review streams cannot keep the process running indefinitely.

The API defaults to `127.0.0.1:8080`. Set `FRONTEND_ORIGIN` to the frontend's exact origin. For a preview on an occupied machine, `FRONTEND_ORIGIN=http://localhost:5174 cargo run` overrides `.env` without editing it.

## Tokio Console (development)

The `tokio-console` CLI is installed separately from the backend. If setting up
another machine, run `cargo install tokio-console --version 0.1.14 --locked`.

Stop your existing backend, then start one instrumented instance from `backend/`:

```bash
RUSTFLAGS="--cfg tokio_unstable" cargo run --features tokio-console
```

In another terminal:

```bash
tokio-console http://127.0.0.1:6669
```

If your terminal defines an empty `COLORTERM` and the CLI reports a color-option
error, use `COLORTERM=truecolor tokio-console http://127.0.0.1:6669`.

This opens a terminal UI, not a browser page. Inspect task busy/idle time, polls,
wakeups and instrumented async resources while using the frontend. Long idle
periods can simply mean waiting for network/database responses. Tokio Console
is not a system CPU/memory monitor or a per-endpoint latency dashboard; use the
existing request/task duration logs and metrics alongside it.

Telemetry binds only to localhost on port 6669. JSON logging still respects
`RUST_LOG`; its filter is separate from Console's runtime tracing, so normal
`RUST_LOG=info` does not disable console collection. The feature adds runtime
instrumentation overhead and requires a rebuild with `tokio_unstable`. Normal
`cargo run` keeps Console disabled. Do not run a second backend on port 8080.

The four job loops currently run inside `tokio::join!` and will not appear as
four independently spawned tasks. Rig request futures suppress their internal
payload tracing; application logs still report AI operation duration.

Setup follows the [official Tokio Console instructions](https://github.com/tokio-rs/console).

## Schema

One development baseline migration lives in [backend/migrations](backend/migrations/).

Ten application tables: `users`, `auth_sessions`, `jobs`, `question_sets`, `interview_sessions`, `interview_questions`, `interview_answers`, `audit_events`, `embedding_jobs`, and `evidence_embeddings`. Users are interviewers or interviewees. Jobs own reusable question sets; generation attempts, retries and leases live only in `question_sets`. Sessions reference `question_set_id`; questions reference `set_id`. Answers are scoped by both session and question. Per-candidate generation, question copies, organizations and memberships are absent.

The development database was rebaselined with existing records preserved. Fresh databases use `cargo run -- --migrate`. Older checkouts with the previous five-migration history require a deliberate development rebaseline or a fresh database; rewriting applied migrations is not a production upgrade strategy.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
ALLOW_REGISTRATION=true cargo test --tests --locked -- --ignored --test-threads=1
```

Integration tests load `.env`, prefer `RUNTIME_DATABASE_URL`, and add uniquely named test accounts/jobs/sessions. Use a dedicated database when testing on shared infrastructure. Tests cover authentication, revocation, interviewer ownership isolation, recruiter/candidate access boundaries, idempotent creation, cursor pagination, private candidate capabilities, guarded transitions, immutable audit records, composite ownership foreign keys, and concurrent expiry workers.

## Backend layout

One `Cargo.toml`, one `Cargo.lock`, and one executable (`platform-api`):

```text
backend/
  src/
    controllers/     # HTTP routes and DTO mapping
    services/        # Business rules and background workflows
    repositories/    # SQL and database projections
    models/          # Internal types
    dto/             # API request/response contracts
    ai/              # Rig adapters and bounded transport
    workers/         # Four polling loops and shutdown
    app.rs           # Central router
    infrastructure.rs # Configuration, tracing and database pool
  tests/             # Integration tests
  migrations/        # Single development baseline
```

`cargo build --release` produces `target/release/platform-api`. `cargo run` starts the entire backend. `cargo run -- --migrate` applies migrations and exits.



Interviewers sign in with email and password directly. The interviewer account manages interviews and reviews evidence. Candidates use invitation links. There is no owner account, workspace-ID entry, or organization-creation form. Ownership is derived directly from the authenticated interviewer.

## Saved invitation links

`INVITATION_ENCRYPTION_KEY` in `.env` must contain a stable 64-character hex key (`openssl rand -hex 32`). The local configuration has been provisioned privately. Keep this key backed up separately from PostgreSQL; do not change it without a key migration. New invitation secrets are stored encrypted and can be retrieved by the authorized recruiter from session details. Existing hash-only invitations require an explicit replacement before acceptance. Retrieval and replacement are audited; expiry is preserved. This enables manual resharing, not automatic email delivery.

After changing backend code, stop the running cargo process with Ctrl-C and start `cargo run` again. Rust is not hot-reloaded by Vite. The timed room uses the existing schema; no additional migration is required.

## Private answer recordings

Candidates can attach short audio/video recordings. `MEDIA_STORAGE_DIR` defaults to `backend/storage/media` and is gitignored; back it up alongside the database. Clips are served only through authenticated APIs, with content-hash checks. Browser capture stops at 60 seconds, server uploads are limited to 15 MB and 12 recordings per interview. Completed unsent clips recover through IndexedDB. Written answers drive AI scoring; media-only answers remain for human review. No migration is needed.

## Interviewer-entered Job IDs

Job creation requires `job_code`, shown as Job ID in the UI (for example `BE-2026-001`). Codes are trimmed, normalized to uppercase and unique within the interviewer account. Use 1–64 ASCII letters/digits/hyphens/underscores, starting with a letter or digit. The internal UUID remains the foreign key used by invitation and question-bank routes. Fresh databases use the single development baseline via `cargo run -- --migrate`.

## Frontend setup

Independent React/TypeScript application. Requires Node 22.12+ (or a supported newer LTS). No Rust, database, or backend checkout is needed to build it.

```bash
cd frontend
# Copy the example only if you do not already have an .env file.
cp -n .env.example .env
npm ci
npm run dev
```

For local development, leave `VITE_API_BASE_URL` empty. Vite proxies `/api` and `/health` to `http://127.0.0.1:8080`, so `localhost`, `127.0.0.1`, and alternate frontend ports work without browser CORS problems. Override `API_PROXY_TARGET` only when the backend uses another address. Restart Vite after editing environment settings.

For separately deployed production services, set `VITE_API_BASE_URL` to the HTTPS API origin and configure the backend's `FRONTEND_ORIGIN` to the exact frontend origin. Vite's development proxy is not included in the production build; a same-origin production deployment must configure its own reverse proxy. Frontend environment values are public, never secrets.

```bash
npm run lint
npm run build
npm run preview
```

The UI supports interviewer registration with email/password, direct login, and logout. The same interviewer account can set up interviews and review candidates. There is no owner role, organization-name form, or workspace-ID login step.

Interview setup creates jobs and candidate invitation links. Review shows session details, pinned job requirements, and paginated audit history. Session lists support local search/status filters over loaded pages, plus pagination and refresh. Dashboard counts come from server totals.

Candidates use private invitation links and can accept an invitation. Question set preparation and retries are available from the job card. Interviewees can start a prepared interview, answer one question at a time, revisit questions, and submit. Server-authoritative time and saved revisions survive refresh. Text/scenario and code answers use an accessible textarea editor; code is not executed. Interviewer tokens are kept in per-tab sessionStorage so refresh preserves login. On reload, /api/me validates the token and supplies identity and role. Expired/revoked tokens are removed; network failures offer reconnection without discarding the stored session. Logout revokes the server session and clears browser storage. Closing the tab ends local persistence. Passwords are never stored. Registration depends on backend policy.

If port 5173 is occupied, use `npm run dev -- --port 5174` with the default same-origin proxy. Do not stop unrelated applications.

Deploy the dist directory on a static host. Configure VITE_API_BASE_URL before building and configure HTTPS and the API origin allowlist independently.

Session details provide larger, darker text and a saved invitation link action. Retrieve and copy an active invitation link to share again. Unaccepted legacy invitations can create a replacement, with an explicit warning that the old link is invalidated. Email delivery is not connected.

Active saved invitation links load directly when session details open. Older API responses without invitation flags use state/deadline checks, instead of incorrectly labelling the invitation expired. Restart the API after backend changes; cargo run does not hot reload Rust code.

Candidate invitation URLs open automatically, without a second Open invitation button. The token stays in the URL fragment (never sent with HTTP page requests), making refresh/back/forward resumable. Hash changes switch candidate context even when a recruiter page is already open. Candidate access never requires a recruiter password; invalid/expired links show a clear error with retry.

The room autosaves after a short typing pause and before navigation/submission. Unacknowledged drafts are journaled in localStorage per interview/question, then removed after matching acknowledgement. Refresh recovers them; conflicting saves from another tab require an explicit choice. The overall timer continues offline, while per-question elapsed/suggested time is displayed. At expiry the server submits acknowledged answers. Locally retained drafts are shown separately if they did not reach the server. Interviewers can inspect saved answers alongside each rubric.

## Backend layers

HTTP features and background workflows now use separate controllers, services,
repositories, internal models and API DTOs. Services own business decisions and
transactions; repositories execute domain SQL and decode query rows.
No database migration, dependency change or frontend change is needed.

### Directory map

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

### Migrated HTTP flows

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

### Background workflows

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

### Ownership and storage


`users.role` has exactly two values: `interviewer` and `interviewee`. Interviewers sign in with email/password, create jobs and invitations, and review evidence. Reviewing is an interviewer capability, not a separate user role. Interviewees access their assigned session through private invitation links. An invitation creates or reuses an interviewee profile; it does not assign a password. Email is unique within each role.

There are no organizations or memberships. Jobs belong directly to an interviewer. Interview sessions identify both their interviewer and interviewee. Questions belong to reusable job sets; answers belong to a session and question. Forced PostgreSQL row-level security uses transaction-local `app.interviewer_id` derived from authenticated identity. Composite foreign keys prevent cross-interviewer references. The runtime database role cannot bypass RLS.

### Ten application tables

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

### Operational logging


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

## API contract

With the backend running through `cargo run`, open [Swagger UI](http://127.0.0.1:8080/docs) or download the [OpenAPI spec](http://127.0.0.1:8080/openapi.json). Both routes are available in debug builds. The UI loads Swagger assets from jsDelivr.

For interviewer endpoints, run the login operation and enter its returned token under **InterviewerToken** in **Authorize**. For candidate endpoints, enter the invitation token under **InvitationToken**. Enter the raw token in either field. Swagger UI adds the bearer prefix.

There are two user types: interviewer and interviewee. There are no organization IDs, memberships, or workspace-selection endpoints. Errors use `{error, code}`. Success responses disable caching.

### Authentication

- `POST /api/auth/register`: `{email,password}` creates an interviewer, when registration is enabled.
- `POST /api/auth/login`: `{email,password}` signs in an interviewer.
- Both return `{token,user_id,role:"interviewer"}`.
- `GET /api/me` validates an interviewer bearer token and returns `{user_id,role}`.
- `POST /api/auth/logout` revokes that login token.

Interviewees use private invitation capabilities and do not need a password. Invitation access cannot authenticate to interviewer endpoints.

### Interviewer endpoints

All routes below are prefixed `/api/v1`. Ownership comes from the authenticated user, never an ownership ID in the URL.

| Method | Path | Purpose |
|---|---|---|
| GET | `/summary` | Counts for the authenticated interviewer |
| GET, POST | `/jobs` | List jobs / create `{job_code,title,description,request_key:UUID}` |
| POST | `/jobs/{id}/invitations` | Create `{candidate_name,candidate_email,duration_minutes,request_key:UUID}` |
| GET | `/sessions` | List owned interviews |
| GET | `/sessions/{id}` | Details and invitation availability |
| POST | `/sessions/{id}/invitation` | Recover the saved link token, or explicitly replace an unaccepted legacy link |
| GET | `/sessions/{id}/generation` | Inspect the assigned reusable set and candidate answers |
| GET | `/audit` | Audit history, optionally filtered by `entity_id` |

Lists use `limit=1..100` and UUID cursors, returning `{items,next_cursor}`. Job/invitation creation is idempotent per interviewer and request key. Identical retries return 200; new records return 201; conflicting reuse returns 409.

Jobs store their description and competency blueprint directly. There is no JD-version or separate blueprint table. The current API does not edit job descriptions after invitations are created.

### Invitations

Creation returns `{session,invitation_token}`. New invitation tokens are hashed for validation and encrypted for recovery in `auth_sessions`. The UI builds `/#invite=<token>`. The fragment stays in the URL so refresh and same-tab navigation retain interviewee access. Opening the link automatically validates it.

Recovery is interviewer-authorized and audited, never extends expiry, and cannot share expired/submitted interviews. Old hash-only links cannot be reversed: replacing an unaccepted legacy link invalidates it explicitly. Accepted legacy links are not replaced. Retrieval/replacement never sends email automatically. Keep `INVITATION_ENCRYPTION_KEY` privately backed up.

### Interviewee endpoints

- `GET /api/v1/candidate/session`: the assigned session only, without rubrics or other participants' data.
- `POST /api/v1/candidate/session/ready`: accepts the invitation.
- `POST /api/v1/candidate/session/start`: requires prepared questions and sets the deadline once.
- `POST /api/v1/candidate/session/submit`: submits an in-progress interview.

Transitions are guarded and retries do not reset deadlines or duplicate audit events. `GET /api/v1/candidate/room?position=N` returns exactly one question and its saved answer/revision, never the rubric. Omitting position restores the latest served question from the audit trail. Reading before start returns 409; after submission it returns the receipt with no editable question.

`POST /api/v1/candidate/answers/{question_id}` accepts `{content,revision}` (revision 0 for a new answer). Content is bounded to 48000 UTF-8 bytes. Changed answers advance their revision; identical retries retain it and produce no duplicate save audit event. Conflicting edits return 409. The question must belong to the capability's assigned session.

All answer writes, navigation, and submission serialize on the interview row. Server clock time is checked after acquiring the lock. At deadline, an in-progress session becomes submitted at its persisted deadline; unstarted invitations expire. Saved answers remain available to the interviewer, while late writes are rejected. Refresh/reconnect never reset time. Back/next navigation is allowed throughout the overall interview; per-question elapsed and suggested time are shown without separate per-question lockouts.

Scoring, adaptive difficulty, camera/audio answers, fullscreen enforcement, and browser proctoring remain later work.

### Question preparation

Configuration uses `LLM_PROVIDER=openai`, `LLM_MODEL=gpt-5-mini`, `LLM_API_KEY`, `EMBEDDING_MODEL=text-embedding-3-small`, and `EMBEDDING_API_KEY`. `LLM_BASE_URL` defaults to `https://api.openai.com/v1`.

When AI is configured, creating an invitation automatically queues question preparation in the same transaction. Opening or accepting an older, unprepared invitation also queues it once. Repeated candidate polls do not duplicate jobs, regenerate prepared questions, or reset failed attempts. Failed generation still requires an explicit interviewer retry. With missing configuration, the candidate sees a setup message rather than a promise of progress.

A preparation request moves the session's generation status through pending/running/completed/failed. A five-minute lease, renewed every 30 seconds during preparation, and a fencing token support recovery. Failures retry up to three attempts; explicit retry resets the attempt budget. Schema, evidence quotations, rubric totals, and embedding output are validated before publishing. Exact fingerprints and pgvector cosine similarity reject repeats across sessions for the same job. Each question stores its rubric snapshot and generation provenance.

Provider request formats follow the official [Structured Outputs](https://developers.openai.com/api/docs/guides/structured-outputs?api-mode=responses) and [embeddings](https://developers.openai.com/api/reference/resources/embeddings/methods/create) documentation. No raw provider response or API secret is written to audit records.

#### Browser observation capture
`POST /api/v1/candidate/events` accepts 1–40 `{id: UUID, kind, client_time: epoch_ms, count?: number}` observations using the invitation bearer capability. Only active interviews can write. Server timestamps are authoritative receipt times; client times and all browser observations are unverified. UUID retries are deduplicated in the immutable audit log. The session row serializes writes and caps storage at 10,000 observations. Paste events contain character counts, never clipboard text. Existing interviewer audit pagination displays the observation evidence.

The browser queues unsent observations on this device and retries after reconnect. This is best-effort capture, not tamper-proof telemetry: client scripts/storage can be modified and cross-tab delivery is not guaranteed. Fullscreen is requested through an explicit browser gesture; denial keeps monitoring active. Device permission denial is recorded when the device button is used. Server receipt signatures, browser single-tab locking and configurable integrity risk points are implemented. Optional recorded media answers are implemented separately; optional face/voice analysis is not included. Dev-tools detection is a window-size heuristic and can be wrong; multiple-display detection depends on browser support.

#### Single active browser tab
The candidate view acquires an exclusive Web Lock named with a SHA-256 invitation digest before mounting any candidate API calls, editor, autosave or device capture. Another tab waits without mounting these features and automatically enters after the owner closes. Browser-managed release handles refresh/crashes without stale local-storage leases. Unsupported browsers show an explicit compatibility message. This guards one origin/profile only; localhost vs 127.0.0.1, separate profiles, browsers and devices are independent. It is not a server authorization boundary. The server continues to enforce answer revisions and deadlines. Lock waiting does not pause time.

#### Rubric assessment
The existing background worker automatically claims submitted interviews, writes an `assessment.started` audit event, and grades all saved answers. At most three attempts are made with a 15-minute claim timeout and fencing against stale workers. Results are stored immutably in `assessment.completed`; no new table is required. Empty answers receive zero without an AI call. Every awarded criterion score must quote an exact substring of the candidate answer and stay within the stored rubric maximum. Candidate content is untrusted data; no tools are exposed to the scoring model. Identity signals are excluded by instruction, and human review remains required.

`GET /api/v1/sessions/{session}/assessment` is interviewer-only and scoped by row-level security. It returns the scoring state, attempts, validated criteria and browser observations with event IDs and server receipt timestamps. The results view polls while waiting and exports its report as JSON. The worker loads AI configuration on startup, so restart after configuration changes. Each validated question result is emitted to the live audit stream while scoring proceeds, with a provisional marker until the whole assessment completes. A competency-weighted profile and evidence recommendation accompany the final result. Explicit retry after expired/exhausted attempts is available; reviewer RAG chat streams separately.


#### Review, adaptive selection and operations
- `GET /api/v1/sessions/{id}/live?after=<event UUID>`: authenticated SSE audit stream, ordered server timestamp/UUID cursor, heartbeat, bounded replay and periodic credential checks. Browser reconnects with the last event ID. Scoring progress is validated per question; provisional progress is not the final assessment.
- `POST /api/v1/sessions/{id}/review`: `{question,conversation:UUID,page:0}`. Embeds only the query, searches persisted session evidence with pgvector cosine distance, and returns six ranked evidence chunks per page plus the exact assessment/integrity summaries (up to eight sources). Chunks contain up to 1,800 characters with 200-character overlap. Indexing pending/failed or query-embedding failure uses session-scoped keyword retrieval without re-embedding documents. Streams actual Responses API text deltas over SSE; partial text is labelled unverified and is discarded on failure. The final `complete` event contains validated exact citations and source text. Up to six prior messages from this interviewer/session/conversation supply context, not evidence. Pages are bounded, so the assistant must acknowledge incomplete evidence; use Next evidence page to inspect more sources. No tools or cross-session retrieval are exposed.
  Best/weakest competency-area questions on page zero compare the latest stored assessment percentages directly, including ties and excluding unscored areas, with no AI/embedding call. Other replies always receive that assessment summary. Model-facing citation IDs are short aliases (`S1`, `S2`); server output resolves them to actual evidence IDs. Whitespace-only quote differences are mapped back to the exact original source substring; unknown sources, altered wording/numbers and empty citation lists are rejected. Integrity questions skip embeddings; other questions use session-scoped keyword retrieval if embeddings fail.
- `POST /api/v1/candidate/recommendation`: `{question_id}`. Saves remain explicit through the existing answer API. Scores the current answer provisionally and chooses an unserved prepared question nearest the target difficulty: >=8/10 harder, <=4/10 easier, otherwise unchanged. Revision checks reject results based on a changed answer. Only the chosen position reaches the candidate; rubric scores remain private. Final scoring re-evaluates submitted answers.
- `GET /api/v1/sessions/{id}/rag`: interviewer-only indexing state (`awaiting_scoring`, `pending`, `running`, `completed`, `failed`), attempts/max attempts, revision/indexed revision, configured model/dimensions and safe error text.
- `POST /api/v1/sessions/{id}/rag`: explicitly retry/rebuild after scoring; resets the four-attempt budget while reusing unchanged chunk vectors. Returns 409 before completed scoring or while an unexpired worker lease is active. See [RAG workflow](#persistent-evidence-retrieval).
- `GET /api/v1/sessions/{id}/integrity`: configurable weighted risk points and exact event evidence, latest reviewer annotation and signature-chain verification. Set optional `INTEGRITY_WEIGHTS` JSON (known keys, integers 0–20) to adjust review priorities. A score is not a probability or cheating verdict. Default weights are returned in each report.
- `POST /api/v1/sessions/{id}/annotations`: `{event_id,decision,note}`. Decisions: `dismissed`, `suspected`, `confirmed_activity`. Notes and actor/time are appended, never overwritten. Confirmed activity does not mean confirmed cheating.
- `POST /api/v1/sessions/{id}/assessment/retry`: starts a new three-attempt budget only after any current 15-minute claim has expired. Original attempt history remains intact.
- `GET /api/v1/sessions/{id}/report`: full JSON hiring record with prompts, rubric snapshots, saved answers, scores, integrity evidence, annotations and audit IDs. Export is logged. Treat exports as private candidate records.
- `GET /api/v1/jobs/{id}/analytics` returns per-question metrics for completed question sets. See [Question-bank analytics](#question-bank-analytics) for the formulas and sample requirements.
- `GET /api/v1/metrics`: interviewer-scoped generation/scoring p50 and p99, session counts, observation counts/rates and scoring failures, computed from immutable server timestamps. Existing request traces plus assessment duration logs omit candidate answers and API keys.

Proctor events carry monotonic server sequence, previous signature, authoritative receipt time and a domain-separated HMAC using a key derived from the stable invitation key. Reusing an event ID for changed content returns 409. Legacy events remain visibly unsigned. These signatures detect changes to received records; a browser controlled by the candidate can still suppress or fabricate telemetry before receipt. Never label browser observations as trusted solely because the server signed receipt.

Streaming implementation follows [official OpenAI documentation](https://developers.openai.com/api/docs/guides/streaming-responses). Partial output is not citation-verified until completion.


#### Optional recorded answers
- `POST /api/v1/candidate/questions/{question}/recordings/{recording UUID}`: raw audio/video bytes with `Content-Type` (`audio/webm`, `video/webm`, `audio/ogg`, `audio/mp4`, `video/mp4`; codec parameters allowed). Container header and size are checked. Up to 15 MB per clip, 12 clips per interview. New uploads require the assigned question and an active interview both before and after file persistence. Identical retries return the same audit reference even after submission; changed content under the same ID returns 409.
- `GET /api/v1/candidate/recordings` and `GET /api/v1/candidate/recordings/{id}`: private own-session metadata and bytes, authenticated with the invitation capability.
- `GET /api/v1/sessions/{session}/recordings` and `GET /api/v1/sessions/{session}/recordings/{id}`: interviewer-only metadata and playback. Files never have public URLs or URL credentials. Playback fetches an authenticated blob and verifies the stored SHA-256 before delivery.

Browser MediaRecorder stops at 60 seconds (or a conservative size threshold), stores a completed blob in IndexedDB, then attempts upload. Interrupted uploads can retry after reconnect or reopening the tab; unsent clips can be downloaded or explicitly discarded. Pending recordings block manual navigation/finish. The overall deadline continues and can end a recording; a late clip remains local rather than silently entering the submitted assessment. The submitted receipt offers local downloads for clips that may not have reached the server. An unfinished recording lost in a browser crash is not guaranteed recoverable; stopped recordings are journalled before upload.

Storage is `MEDIA_STORAGE_DIR`, default `backend/storage/media`, outside static frontend assets. Files use owner/session/recording UUID paths and Unix mode 0600; lightweight references, content hashes and timestamps use existing append-only `answer.recorded` audit events. Cancelled uncommitted files can be repaired on retry under a session lock. Back up the private media directory with the database. There is no new table or migration. JSON report exports carry references, not embedded media files.

Recordings count toward answered-question progress. AI scoring uses written answers only; a media-only question is marked `media_requires_human_review` with a null score and excluded from numeric AI totals/weighted competency coverage. Reviewer chat can retrieve attachment metadata, but cannot interpret the recording contents without a written transcript.


#### Generation v2: worked reference answers and publication gates
New question sets use `jd-questions-v2`. Existing prepared question/rubric snapshots are preserved. New blueprints check distinct competencies and prevent severe allocation drift from JD weights while preserving one-slot coverage.

A candidate draft must match the blueprint, fit the overall interview duration and include:
- A worked reference explanation (not a generic instruction to explain), assumptions, defensible alternatives and concrete common mistakes.
- Two to six distinct observable rubric criteria totalling ten points. Each has different partial/full-credit anchors and a substantive criterion-keyed reference answer. The server assembles the worked reference and links each criterion to its reference section deterministically.
- For coding tasks only: language, a reference solution, complexity analysis and both typical and edge-case input/output examples. Reference code is not executed. A separate model review recomputes examples and checks plausibility; this is not a formal correctness proof.

Each draft passes deterministic checks, embedding validation, in-set and whole-JD-bank duplicate checks, then a separate structured quality review of every question. The critic checks JD relevance, answer correctness, rubric alignment, timing, ambiguity and answer leakage. A failed check cannot publish partial questions. One bounded in-claim repair draft receives the rejected draft, question-specific failure feedback and actual colliding bank prompts, followed by the existing three-attempt durable retry policy. A final advisory-lock-protected duplicate check handles concurrent sessions before atomic publication. Extended model calls retain their lease through fenced heartbeat updates.

Rubric JSON stores the worked answer, alternatives, pitfalls, coding examples, criterion anchors, estimated minutes and per-question quality-review result alongside generator/model provenance. Interviewer details expose this information; candidate APIs continue to exclude all answer-key material. Scoring accepts criterion-satisfying alternatives rather than requiring the reference wording. AI review reduces weak outputs but cannot guarantee factual or executable correctness; reviewers should inspect the evidence and reference examples.

Strict schema normalization marks nullable properties as required and allows explicit null values, matching the provider contract. Operational errors expose only whitelisted status/code information, never raw provider messages. Authentication, schema and quota failures stop promptly; transient rate-limit/server failures retain the durable retry path.


Blueprint generation has its own bounded repair pass. New internal competency IDs are normalized deterministically and all slot references are remapped together; empty, duplicate or unknown IDs are rejected. Accepted stored blueprints are not rewritten.

Question/reference generation has a separate per-model-request timeout: `GENERATION_REQUEST_TIMEOUT_SECONDS`, default 180 seconds, clamped to 30–300. Other existing requests retain their prior timeouts. Long generation calls retain worker lease renewal and finite repair/attempt budgets.

Generation reference sections are authored directly under their criterion IDs, rather than asking the model to copy quotes from its own prose. Publication compiles `expected_answer` from `reference_overview` plus `reference_sections`, and stores each criterion’s reference text/source link. These are generated answer-key materials checked by a separate critic, not candidate evidence. Candidate-answer quotes used for scoring remain strict exact-substring checks. Schema-level length/range/array bounds guide generation, and equivalent server-side checks remain authoritative.

Strict schema requirements follow [OpenAI’s structured-output documentation](https://developers.openai.com/api/docs/guides/structured-outputs), including required nullable fields and supported value constraints for the configured base model.

Generation request schemas are serialized in authoring order (question, worked answer, then rubric), following the provider’s documented schema-key ordering behavior. This request-only serializer does not alter the canonical JSON serialization used for stored audit signatures.

Generation schemas enforce the exact requested number of blueprint slots, questions and quality-review results using equal min/max array counts. For the configured `gpt-5-mini`, blueprint/question drafting defaults to low reasoning effort (`GENERATION_DRAFT_REASONING_EFFORT` can override minimal/low/medium/high); independent quality review explicitly uses medium effort. Other configured model IDs retain their original request settings. The question-set output budget is 20,000 tokens to accommodate worked answers without dropping questions. Reasoning controls follow [official OpenAI guidance](https://developers.openai.com/api/docs/guides/reasoning).

#### Human-readable Job IDs
`POST /api/v1/jobs` requires `job_code`. The API trims and uppercases it; codes are 1–64 ASCII letters, digits, hyphens or underscores and must start with a letter or digit. The database enforces uniqueness per interviewer, so case/whitespace-equivalent duplicate creation (including simultaneous requests) returns 409. Identical retries using the same request key remain idempotent; changing the Job ID under an existing key returns 409. Another interviewer may use the same code independently. Job lists, session lists/details, candidate session data and JSON reports include `job_code`; internal `id`/`job_id` UUIDs and route parameters are unchanged. Existing jobs receive deterministic `LEGACY-…` codes through one additive migration.


### Media proctoring evidence

`POST /api/v1/candidate/events` additionally accepts the media observation kinds and bounded optional `measurements` object documented in [media proctoring](#cameraaudio-proctoring). Duplicate event IDs must retain the same measurements.

- `POST /api/v1/candidate/events/{event}/evidence/{asset}`: binary JPEG/audio/video body, candidate bearer token; binds an attachment to a recent allowed event. Retry with the same asset UUID and identical bytes.
- `GET /api/v1/sessions/{session}/evidence/{asset}`: owning interviewer only; verifies receipt signature and content hash before returning private media.
- `GET /api/v1/sessions/{session}/integrity`: each observation now includes an `attachments` array with asset IDs, MIME types, byte counts, timestamps, hashes and signatures. Existing risk/annotation fields remain.

Limits and model/calibration behavior are in the linked guide. Evidence attachments do not count as candidate answers.

#### Job question-set pool

`POST /api/v1/jobs` accepts `preparation_duration_minutes` (5–180; default 30) and `question_set_count` (1–5; default 1), and atomically queues that many reusable sets. `GET /api/v1/jobs/{job}/question-sets` returns owner-only status and questions/reference rubrics; `POST` retries failed sets. Invitations require a completed matching set and link it without copying questions or generating replacements. See [details](#ai-workflows-and-rig).

#### Incremental generation progress

Generation v4 saves fully validated questions individually. Owner generation/status and pool previews may show saved questions while status is running/failed. Partial rows do not imply completion. Candidate session responses include `questions_ready`; starting requires a completed assigned set. `POST /jobs/{job}/question-sets` retries failed sets after their saved positions. Pool status includes owner-only `last_failure` summaries/findings, without the raw diagnostic draft.

#### Invitation readiness

Creating a candidate invitation requires a completed, nonempty question set for the same job and duration, within the configured set count. Otherwise the API returns HTTP 409 and rolls back candidate/session/token creation. A successful invitation assigns that reusable set immediately. Replaying an existing request key still recovers the original invitation. The interviewer UI disables invitations until a ready set is available. Existing historical waiting invitations are preserved.

#### Candidate Next resilience

`POST /api/v1/candidate/recommendation` saves no answers itself: the room flushes the current draft first. A valid provisional grade for the same question/answer revision is reused. Otherwise written-answer grading is capped at six seconds; missing AI configuration, provider/validation errors and timeouts fall back to prepared question order. Empty/media-only answers skip provisional AI grading. A revision change during the request discards the stale grade and uses normal navigation rather than returning a conflict. Active-session, ownership and deadline checks remain mandatory. Once every question has been visited, Next advances by position; beyond the last position it returns null and the room asks for submission confirmation. No scores/reference answers are returned to the candidate.

Scoring recovery: one initial attempt plus three retries. Explicitly failed attempts
release their claim immediately and retry after a 15-second backoff; the 15-minute
lease applies only to attempts with no terminal outcome (for example a crashed
worker). Manual retry resets the budget and bypasses prior failure backoff. Validated
per-question checkpoints are reused only when their input hash (answer, prompt,
rubric, recordings, model and scorer version) still matches. Older checkpoints with
no input hash are scored again. The assessment endpoint exposes `max_attempts` and
`last_failure` (safe reason and question position), which the review UI displays.
Embeddings remain queued only after the entire assessment completes successfully.

## AI workflows and Rig

### Provider integration

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

### Reusable sets and assignment


Creating a job atomically queues the interviewer-selected 1–5 question sets (default 1). The interviewer chooses a preparation duration of 5–180 minutes. Each set uses the job blueprint and sequential question/reference/rubric generation with validation, embedding deduplication and independent review. New questions are scoped to 2–3 minutes, and their count is planned to fit the duration.

The Question sets panel shows status, saved progress, reference answers, time plans and bounded failure details. Generation runs only for job sets. There is no per-session generation fallback or automatic replacement on assignment.

Invitations require a completed, nonempty set matching the duration. The transaction links `interview_sessions.question_set_id` to a reusable set, balancing usage across configured sets. Every question belongs to `interview_questions.set_id`; questions are never copied per candidate. Answers and progress remain scoped by session and question. Prepared sets remain usable with the AI provider offline.

Generation state, initial attempt plus three retries, next-run time and worker leases live only in `question_sets`. Retries preserve saved question checkpoints. After four failed worker attempts, an interviewer can explicitly retry the job's failed sets.

### Storage and API

The single development baseline is `202611040001_initial_schema.sql`. Question text, worked references, rubrics and embeddings are stored in `interview_questions`; candidate answers are in `interview_answers`. The SECURITY INVOKER `session_question_details()` function projects questions through session assignment and preserves row-level security.

- `POST /api/v1/jobs`: accepts `question_set_count` and `preparation_duration_minutes` alongside the job fields.
- `GET /api/v1/jobs/{id}/question-sets`: interviewer-only set status, questions and references.
- `POST /api/v1/jobs/{id}/question-sets`: explicit retry of failed configured sets.
- `GET /api/v1/sessions/{id}/generation`: inspect the assigned set and that candidate's answers; this endpoint does not schedule generation.

No candidate API returns reference answers or rubrics.

### Generation policy

### Automatic retry policy

Every generation failure now receives up to three automatic worker retries after the initial attempt (four total attempts), including validation rejection, duplicates, provider errors, unexpected response formats, and unexpected preparation panics. Retry delays are 5, 10 and 15 seconds. Task attempts and next-run times are stored in PostgreSQL, so restarting the backend does not reset the budget. A failed preparation task cannot terminate the worker loop; cancellation also aborts its in-flight work.

Saved checkpoints and the time plan are reused. The next worker attempt receives persisted review findings and the previous rejected draft. Existing bounded local draft/repair calls remain inside each worker attempt, so four worker attempts does not mean only four provider requests. After the fourth failure, the set requires an explicit interviewer retry. Existing failed records are not silently restarted by this code change. No live-provider tests were run.

### Time planning

Before drafting any question, the backend plans all remaining slots and persists the plan as a `generation.planned` audit event for the set. New questions get fixed allocations of 2–3 minutes. A 10-minute fresh interview uses at most five 2-minute questions; a 30-minute interview with six questions uses up to eighteen task minutes, leaving review time. Priority for a third minute combines answer type and difficulty. Code scope is one small function or a focused fix in supplied starter code (normally 3–8 changed lines); scenario/text scope is one decision/concept answered in 2–4 sentences. Full components, services and multi-feature workflows are excluded.

The blueprint request count is capped by duration / 2. Older cached blueprints are adapted locally to fit, keeping higher-priority competencies. Existing saved questions and their times are preserved. A legacy partial set may therefore retain some earlier one-minute questions; only new questions follow the 2–3 minute policy. A set-specific plan is reused across retries, so cached job blueprints and saved questions do not need paid regeneration. The strict output schema fixes estimated_minutes to the planned allocation. Quality review checks scope and correctness independently. A time plan does not guarantee that every person will finish in exactly that time.

The interviewer can expand Question time plan to see the number of questions, each allocation and scope, and the remaining review time. No new database table or migration is required.

### Per-question generation and API usage

For each unsaved position, generate the complete question, reference answer and rubric in one call. Validate its structure, time allocation and exact fingerprint in Rust before requesting an embedding. Check semantic similarity against the job bank, then use one independent content review for both prompt and reference. Save only the fully validated question before proceeding.

A successful six-question set uses 6 draft calls + 6 embedding calls + 6 review calls, plus one blueprint call when the job has no cached blueprint. Each question allows one duplicate replacement and one content repair at most (three drafts maximum); unchanged prompts reuse their embedding during content repair; it does not regenerate previous checkpoints. Invalid local drafts and duplicates skip review. Transient service errors can still trigger the existing bounded task retries, so total usage can be higher on failures. No live AI test was run for this change.

The combined draft may spend answer tokens on a question that is subsequently rejected as a duplicate. This trades some wasted tokens on duplicate drafts for fewer requests on successful questions.


### Scoring and evidence indexing

Scoring validates rubric criterion coverage, point bounds and exact answer quotes.
Blank answers receive zero; media-only responses require human review. Saved
checkpoints are keyed by answer, rubric and model inputs. Submission queues durable
scoring; completed scoring queues evidence indexing. Adaptive navigation bounds
live grading to six seconds and falls back to sequential navigation.

Reviewer streaming retrieves only the current session's evidence, validates final
citations and persists verified final replies. Deltas remain unverified until
completion. Integrity summaries are deterministic counts/weights. See [RAG](#persistent-evidence-retrieval)
for chunk storage and retrieval and [API](#api-contract) for status/manual retry routes.

### Verification limits

Local/mock tests cover transport bounds, malformed responses, refusal handling,
embedding ordering, scoring validation and concurrent stream isolation. Historical
live generation trials encountered quality rejections; those tests do not establish
full provider acceptance or a generation speed guarantee. Scoring rationale is not
streamed token by token. No live provider calls are needed for documentation edits.

### Question-bank analytics

Metrics are grouped by the original question ID across interviews that reuse the same question set. Score metrics use the latest completed assessment for each interview.

| Metric | Calculation |
| --- | --- |
| Average rubric score | Average of `10 × earned points / maximum points` across scored responses. For a 10-mark question, 1.5/10 means 1.5 marks on average, or 15% of available marks. |
| Attempts per question | Count of submitted interviews with a recorded visit to the question before submission or the deadline. Revisits count once per interview. |
| Average time per question | Sum each interview’s question visits, then average those totals across timed interviews. Each visit runs from question open to the next question open, submission, or deadline, whichever comes first. This includes idle time. |

#### Score separation

For each comparable interview, calculate two values:

- `xᵢ = points earned on this question / maximum points for this question`
- `yᵢ = points earned on the other scored questions / total maximum points for those questions`

A comparable interview has a valid score and positive maximum for this question and at least one other scored question. Pearson correlation compares those paired values:

```text
r = Σ[(xᵢ − x̄)(yᵢ − ȳ)] / √(Σ[(xᵢ − x̄)²] × Σ[(yᵢ − ȳ)²])
```

Here `x̄` and `ȳ` are the averages across comparable interviews. A value closer to +1 means candidates who scored higher on the rest of the interview tended to score higher on this question. Values near zero indicate little linear association. Negative values indicate a reverse pattern.

The calculation requires at least two comparable interviews and variation in both score series. With one pair or a constant score series, the denominator is zero. With exactly two varying pairs, the result is always +1 or −1. The chart plots valid correlations as soon as they exist and shows a descriptive label in the question tooltip. Separation describes rubric performance within the interview.

## Persistent evidence retrieval

The flow is `interview submitted → scoring completed → embedding job → evidence search ready`. Scoring and indexing are separate outcomes. Existing completed assessments are queued automatically when the embedding worker starts; they are not scored again.

`embedding_jobs` stores one job per session, revision/indexed revision, status, attempts, available time and a fenced five-minute lease. `evidence_embeddings` stores source IDs/types, original readable chunk text, SHA-256 content hash, pgvector vector, model and dimensions. Answers, assessment results and events are session scoped. Prompt/rubric chunks are question scoped and reused by all candidates assigned that question set. Question-generation deduplication vectors remain separate because those describe prompts rather than answer/rubric evidence.

Database triggers queue the job atomically when a completed assessment is inserted. Late proctoring events, attachment metadata, reviewer annotations, answer-content changes and shared question/rubric updates invalidate the affected index. New revisions reset their retry budget. Raw media files are not embedded or automatically transcribed; event text links them to their audit evidence IDs.

The worker chunks text into up to 1,800 Unicode characters, overlapping by 200. It checks cached hashes for the same owner/model/dimensions and embeds only missing/changed text, in batches of at most 32. Each saved batch is a checkpoint. Duplicate text in a batch needs one embedding. Repeated rubric/prompt chunks are reused across interviews without duplicating their stored vectors.

A run gets one attempt plus three automatic retries, with 5/10/15-second backoff. Provider failures, validation failures and persistence errors do not change `interview_sessions.state` or assessment results. An interrupted worker is reclaimed after its lease expires; earlier saved batches are reused. Publication checks the current revision and claim token; a concurrent source change schedules fresh work rather than marking stale content ready. An exhausted job remains failed until explicitly retried or new evidence changes its revision. Model/dimension changes queue a new index; mismatched vectors are never queried together.

Chat checks readiness, embeds only the interviewer’s question and searches the entire eligible index before pagination. Each page has six ranked chunks, leaving room for exact SQL integrity counts and the stored competency summary. Source IDs remain attached to text and all final model quotes are checked against those sources. Keyword fallback loads this session’s current raw evidence and does not regenerate embeddings. Best/weakest competency comparisons and integrity counts avoid embedding calls. The UI displays index status and a retry control separately from chat.

RLS enforces interviewer ownership. Retrieval additionally accepts only this session’s answer/event/assessment chunks plus question chunks belonging to its assigned set. Another candidate’s answers are excluded even when both candidates share a question set. pgvector uses exact cosine search over the scoped subset; there is no approximate global index or cross-session top-k filtering. Session/question deletion cascades to its dependent index rows.

### Development schema

The baseline migration `202611040001_initial_schema.sql` includes the RAG tables, indexes and queue triggers. The job deletion migration grants the application role permission to delete job data. Apply pending migrations with `cargo run -- --migrate` from `backend/`.

### Focused verification

`cargo test --lib rag::documents` checks stable Unicode chunking and readable quoting. The ignored `rag::tests::durable_embeddings_reuse_checkpoints_recover_and_remain_session_scoped` test needs a disposable migrated database in `RAG_TEST_DATABASE_URL`, using the restricted application role. It starts its own local embedding mock and never loads provider secrets or calls a live AI service. It checks post-scoring queueing, saved-vector reuse, query-only embeddings, partial-batch recovery, stale leases, source updates, shared rubrics, pagination, owner/candidate isolation and exhausted retries preserving scores. Remove that disposable database after the run.

## Camera/audio proctoring

### Candidate and interviewer flow

The invitation page requests camera/microphone access and records permission outcomes. Local checks start automatically for enabled devices during the interview. The camera panel shows live face count and calibration status. Device denial/off events are recorded; browser monitoring continues. Fullscreen and explicit screen sharing are required for the room, with screenshots of the selected surface linked to browser and media flags.

For eye/head calibration, look toward the camera for approximately eight seconds of valid camera samples; brief blinks are tolerated and gaps over 2.5 seconds restart calibration. Recalibration is available in the panel. Low light, glasses, camera angle, assistive equipment and frame rate affect estimates. This is coarse geometry, not point-of-regard tracking or an assessment of attention, emotions, intent or ability.

The interviewer opens the session's Integrity review → **Camera and audio observations**. Each item includes a timestamp, numeric measurements, receipt-chain status and private snapshot/clip controls. Existing annotation controls dismiss or qualify observations. SSE refreshes the integrity view as events and attachments arrive. JSON exports include attachment IDs, hashes and signed metadata; binary media is fetched separately with interviewer authentication.

### Local processing and thresholds

MediaPipe packages are pinned to `0.10.32`. Face Landmarker and YAMNet files are downloaded from Google's versioned official model URLs and checked against SHA-256 hashes in `frontend/scripts/proctoring-models.json`. Camera frames and PCM samples are processed in classic Web Workers, not sent to an AI/cloud endpoint. `AudioWorklet` collects one-second mono PCM windows. Workers retain no recognition templates or face identity data; raw landmarks are not uploaded. Expression blendshapes are disabled.

| Observation | Gate | Default risk points |
| --- | --- | --- |
| `face_missing` | Zero detected faces for ≥2 seconds | 1 |
| `multiple_faces` | More than one detected face for ≥2 seconds; model returns at most three | 4 |
| `gaze_away` | Calibrated iris/head offset sustained ≥4 seconds | 0 |
| `possible_background_speech` | Speech probability ≥0.65, plus a fresh camera result with no face, multiple faces, or low visible mouth motion, sustained ≥2 seconds | 1 |
| `speech_detected` | Speech ≥0.65 for ≥2 seconds when fresh camera evidence is unavailable | 0 |

These are heuristic observations. Speech classification cannot determine whether a voice belongs to the candidate, another person, TV/audio playback or a synthetic source. Mouth-motion sampling can miss movement and does not establish speaker identity. This implements **possible** background-speech analysis, not diarization or a reliable count of speakers. The reviewer assistant is explicitly instructed against attention/emotion/intent/identity inference from these signals.

Each signal has a 60-second cooldown. Sampling gaps over 2.5 seconds reset sustained-condition gates. Hidden/paused video is not treated as a fresh missing-face result. The face model samples up to five times per second, with one inference in flight; inference runs off the UI thread. Gaze requires both open/visible eyes and calibration; horizontal iris change >0.18 eye-width units, vertical change >0.09 eye-width units or horizontal head offset >0.16 face-width units / vertical head offset >0.12 face-height units can trigger the gate.

Risk points are configurable with `INTEGRITY_WEIGHTS`, remain separate from rubric scores, and are capped at 100 as review priorities. A flag, model probability or signed receipt is not proof of cheating.

### Evidence capture, storage and recovery

After a sustained media flag, save an aspect-preserving camera JPEG (up to 640 pixels wide) and a shared-screen screenshot if sharing is active after fullscreen entry. Face/look-away flags get a three-second post-signal audio/video clip. Concurrent face flags share one recording, each linked to its own observation. Speech/background-speech flags save up to three preceding seconds as mono PCM16 WAV from the same microphone windows used for classification. Only three one-second windows are held in memory, discarded on stop; no continuous audio is uploaded. If the speech buffer is unavailable, a post-signal clip is attempted. Each kind has a separate 30-second capture cooldown; different flags do not suppress each other's snapshots or speech clips.

Evidence uses the existing `audit_events` table plus private files under `MEDIA_STORAGE_DIR`; no new tables or migrations were added. Camera/audio files have an `.evidence` suffix; screen JPEGs use a session `screenshots/` directory and are separate from answer recordings. They never count as answers and never enter automated answer grading.

The backend requires an authenticated candidate and an existing recent `proctor.observed` event in that candidate's session, of an allowed media-signal kind. Limits: 512 KiB JPEG, 2 MiB per attachment, one image plus one clip per event per source (screen/camera), 40 camera/audio attachments and 120 screen screenshots per session, and a 15-minute attachment window after server receipt of the event. PCM WAV format/length is validated server-side (mono 16-bit, at most three seconds). Compressed clip duration is limited by the browser; the server checks size/signature/file markers but does not decode codecs or verify duration or scene authenticity. A late attachment can finish after submission if it links to a previously accepted event within that window; new observations still require an active interview.

Attachments include the event ID, parent receipt signature, content hash and their own HMAC receipt. Private reads verify both the signed metadata and file SHA-256. Reads require the owning interviewer; candidates cannot browse proctor evidence using interviewer endpoints. Retries use the same asset/event IDs; changed content conflicts. Receipt-chain verification orders batched events by server sequence, not random UUIDs.

IndexedDB retains unsent observations and attachments for up to 14 minutes, with at most 20 pending items per session. Previously captured items retry on reconnection/refresh independently of starting new media monitoring. Event acceptance is persisted before attachment upload so retries after submission can complete against an existing receipt. Candidates see pending/capture failures. Browser storage denial, quota exhaustion, crashes during capture or deliberately modified clients can cause missing evidence; an observation without an attachment is shown explicitly as unverified. Expired pending items are removed on a later queue scan.

Server evidence has the same lifecycle as the existing private media store: no automatic deletion job is implemented. Operators must apply their retention policy to private files and audit exports/backups. Changing the stable invitation/signing key invalidates old receipt verification; preserve it alongside protected backups. Do not expose the media directory as static files. Server signatures prove received bytes/metadata have not changed, not that client telemetry was complete or truthful.

### Setup and verification

From `frontend/`, run `npm ci` followed by `npm run setup:proctoring`. `npm run dev` and `npm run build` also run setup automatically. First setup needs network access for the two official model downloads (~8 MB); subsequent setup validates cached files and rebuilds the local worker. WASM files are copied from installed packages. Serve generated `public/proctoring` assets on the same origin; Vite copies them into production `dist/proctoring`. They are generated/ignored in Git. A model-load failure leaves browser monitoring operational with a visible unavailable status.

Validation includes deterministic signal tests; backend authorization, idempotency, attachment limits, hash tampering and event-chain tests; a browser model fixture (`frontend/tests/proctor-browser.html`); and synthetic room/capture/playback fixture (`frontend/tests/proctor-ui.html`). Test fixtures are development-only HTML entries and are not part of the production application build.

Browser smoke tests used a blank frame/silence, Google's official portrait/speech test samples, a tiled portrait for two faces, and generated camera/audio tracks for automatic capture. They did not access the user's real camera/microphone. Representative consented real-world tests are still needed to tune false positives across lighting, devices, glasses, disabilities and speaking styles. Positive sample fixtures are temporary test downloads, not production assets.

Implementation references: [Face Landmarker](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker?tab=Web), [Audio Classifier](https://developers.google.com/edge/mediapipe/solutions/audio/audio_classifier?tab=Web).

### Review assistant resilience

Integrity questions use exact session-wide database counts and bounded keyword retrieval without an embedding call. Other review questions retain semantic retrieval and fall back to session-scoped keyword retrieval if embedding fails. Counts include neutral capability/permission events and repeated observations, so they are not confirmed breaches or unique incidents. The configured LLM is still needed for chat replies, and every cited quote is validated against retrieved sources.

### Analysis readiness and event delivery

Camera permission and `monitoring_started` do not establish that inference is functioning. `face_analysis_ready` is recorded only after the first fresh face result; `gaze_calibrated` after baseline calibration; `audio_analysis_ready` after local microphone analysis starts. These status events and model failures are visible alongside media observations and carry no default risk points. Prolonged look-away still requires calibration and eight sustained seconds; missing face requires five sustained seconds, including when the lens is covered (the detector cannot establish why a face is absent).

All signal durations are whole milliseconds. Browser `performance.now()` is fractional; sending those fractions previously failed integer API validation and prevented flags/attachments from reaching the report. Unaccepted IndexedDB records now normalize only their durations before retry, retaining their IDs and capture timestamps. Upload failures show the HTTP status on the candidate page. Historical flags never accepted by the server cannot be reconstructed after submission.

### Copying question text

A copy selection wholly inside the displayed question is recorded as `copy_from_question`. Pasting that same text while that question is current is `paste_from_question`; only whitespace differences are ignored. Opening the question's context menu is `question_context_menu`. These are permitted activities with zero risk points, even when custom weights are configured, and do not trigger anomaly screenshots. Clipboard text stays in browser memory; only event type and character count are recorded. Unobserved, modified, external or other-question pastes remain normal paste observations.

#### Analysis health and delivery feedback

The candidate view shows live face count, sustained-condition countdowns, speech
confidence, and the most recent flag. A countdown resets when the condition clears
or samples become stale; short movements are not flagged. Hidden tabs and paused
previews show an explicit paused state. A worker that stops producing fresh results
for five seconds is marked unavailable, rather than silently appearing healthy.
Analysis failures trigger at most three automatic restarts per mounted interview
media view. The candidate can also use **Restart face/voice checks**; restarting
requires fresh gaze calibration.

Evidence uploads retry each queued record independently. A rejected record does not
block subsequent observations, and successful attachments are checkpointed before
retrying remaining attachments. Analysis-ready/calibration events are status-only:
they intentionally have no attached screenshots or clips. These changes do not add
speaker identification; possible background speech remains a face/mouth-and-speech
heuristic, not verified detection of another speaker.

### On-demand live screen (WebRTC)

During an active interview, session details offer **Start live view** / **Stop live view**.
The candidate sees a notice while connecting/viewing. The sender reuses the screen
already selected through the browser screen picker and sends enabled camera video
as a separate track. No microphone audio is sent.
Screen evidence capture continues independently. Closing the viewer closes the peer;
stopping sharing or ending the interview closes the sender. One viewer per session.

The Rust API handles authenticated SDP signaling over short HTTP polls (2 seconds),
not media transport. Every poll checks session ownership and the active deadline;
candidate access derives the session from their invitation token. SDP has a 64 KiB
limit. Ephemeral rooms expire after 20 seconds without viewer heartbeats; no database
migration or live video storage is involved. Server restart requires reconnecting.
This in-memory signaling implementation requires one backend process (use a shared
signaling store before scaling to multiple instances).

`WEBRTC_ICE_SERVERS` is a JSON array of browser RTCIceServer objects, default `[]`.
Local reachable peers can connect without a relay. Across NAT/firewalls configure
STUN and **TURN**; STUN alone does not guarantee a connection. Example structure:
`[{"urls":"stun:your-host:3478"},{"urls":"turn:your-host:3478","username":"temporary-user","credential":"temporary-password"}]`.
ICE credentials are sent to authorized participating browsers: use limited-lived
TURN credentials, never unrestricted permanent secrets. Production requires HTTPS.
No public third-party ICE service is contacted by default.

## Assignment implementation status

| Requirement | Implementation |
| --- | --- |
| JD blueprint, seeded unique questions, exact/vector dedup, rubric snapshots | Generation v2: worked references, assumptions, alternatives, mistakes, criterion-keyed reference sections, partial/full-credit anchors, coding examples, deterministic checks, a separate AI critic, bounded repairs and renewed worker leases |
| Adaptive difficulty | Next question evaluates the saved answer and chooses a suitable unserved question from the prepared set; manual question navigation remains available |
| Timed room, cursor resume, text/code/media, autosave | Server deadline, optimistic revisions, local unsent draft recovery, refreshed cursor; optional bounded audio/video attachments with private playback and IndexedDB retry recovery |
| Fullscreen and one browser tab | Editor requires fullscreen; exclusive Web Lock gates the candidate view on one origin/profile |
| Media proctoring | Opt-in local face counts, calibrated eye/head geometry, possible background-speech analysis, event-linked private snapshots/clips and interviewer playback; see [details and limitations](#cameraaudio-proctoring) |
| Browser proctoring | Focus/visibility/fullscreen, copy/paste count, context menu, typing bursts, rapid answer timing, window-size dev-tools suspicion, supported display-extension signal |
| Immutable evidence | Server timestamp, event IDs, idempotency, measurement units, signed receipt chain, RLS-protected append-only audit |
| AI scoring | Rubric point bounds, exact evidence quotes, blank-answer zeroes, per-question live progress, final competency profile and human-review recommendation |
| Reviewer assistant | Scoped embeddings/pgvector retrieval, evidence paging, exact citations, per-conversation memory, genuine token streaming and final citation validation |
| Integrity review | Configurable risk weights, evidence list, activity vs suspicion distinction, audited reviewer overrides, AI evidence-summary action |
| Live reviewer | SSE replay/cursors, heartbeat, credential checks, reconnect and provisional scoring events |
| Analytics and observability | Per-question rubric averages, visit counts, average time and exploratory score separation; scoped latency percentiles, counts, request/worker traces |
| Export | Full JSON report with answer/rubric/audit/score/integrity evidence |
| Bias and injection guardrails | Model receives job-relevant prompts/rubrics/answers, not candidate profile; no tools are exposed. Citations and point limits are validated; human judgement remains necessary |

### Explicit limits

- Optional audio/video answers and opt-in local media proctoring are implemented. Eye/head direction and possible background-speech flags are heuristic estimates, not attention/intent inference, speaker identification or cheating verdicts. Only event-triggered proctor snapshots/clips upload automatically after opt-in. Browser monitoring continues if device/model access is unavailable. Media-only answers still require human review, not an automatic zero. See [media proctoring](#cameraaudio-proctoring).
- Web Locks cannot coordinate different origins, profiles, browsers or devices. They prevent accidental duplicate tabs, not malicious API clients.
- Web apps cannot reliably detect every display or dev-tools instance. Unsupported browser signals are unavailable; window-size heuristics can produce false positives. Typing bursts may come from assistive tools, input methods or accessibility software.
- A server signature proves receipt integrity, not the truth or completeness of client telemetry. Browser-side suppression cannot be ruled out.
- RAG operates on explicit bounded evidence pages, not an unbounded model context. Citations prove that quoted text exists, not that a model's interpretation is necessarily correct. Provisional streamed text is clearly labelled and discarded when final validation fails.
- AI scoring and recommendations support a human reviewer. Rubric/identity bias and statistical validity still need evaluation on representative, consented data; small-sample analytics are not validated discrimination or fairness measures.
- Recording capture/upload/playback and local recovery have been smoke-tested with synthetic video/audio tracks, without accessing real camera/microphone hardware. Live AI integration is covered with a local mock provider. Current provider keys/model availability and real-world camera/browser compatibility need deployment acceptance testing.

Private media files live under `MEDIA_STORAGE_DIR` (default `backend/storage/media`). Back up this directory alongside PostgreSQL and the stable invitation key. Do not expose it through a static web server. JSON exports include audit references and content hashes, not the binary recordings. Browser recording is capped at 60 seconds, server size at 15 MB and server count at 12 clips per session. There is no server-side codec decoding or duration verification.
