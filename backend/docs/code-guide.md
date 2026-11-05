# Code guide and operational logging

The backend is still one Rust package and one executable. Module boundaries follow features; HTTP routing, worker orchestration and validation rules have separate files. No schema or migration changes are needed for this refactor.

## Start reading here

1. `src/main.rs`: environment, database connection, server, CORS and shutdown.
2. `src/lib.rs`: module inventory and public entry points.
3. `src/app.rs`: all HTTP feature routers, health checks, body limits and middleware.
4. A feature's router, then its handler; handlers open scoped transactions through `src/access.rs`.
5. For asynchronous work, follow the scheduling endpoint to the feature worker, then its pipeline/rules.

| Location | Responsibility |
| --- | --- |
| `auth.rs` | Interviewer login, registration, logout, session verification and token primitives |
| `access.rs` | Authenticate interviewer/interviewee and open an owner-scoped transaction |
| `error.rs`, `pagination.rs` | Shared HTTP errors and bounded cursor pagination |
| `audit.rs` | Durable domain event writes and audit listing |
| `jobs.rs` | Job creation, Job ID normalization and listing |
| `interviews/mod.rs` | Session route composition |
| `interviews/staff.rs` | Interviewer session lists/details and dashboard summary |
| `interviews/invitations.rs` | Invite candidates and recover/replace links |
| `interviews/candidate.rs` | Candidate session projection and state transitions |
| `invitations.rs` | Invitation encryption service |
| `interview_room.rs` | Candidate questions, revisioned answer autosave |
| `proctor_evidence.rs` | Private event-linked snapshot/clip uploads and verified interviewer reads |
| `media.rs`, `proctoring.rs`, `adaptive.rs` | Recordings, browser observations, next-question recommendations |
| `generation/http.rs` | Queue generation and read status |
| `generation/worker.rs` | Claim jobs, renew leases, manage retries |
| `generation/pipeline.rs` | Blueprint, draft, validation, critique, repair, duplicate checks and publication |
| `generation/provider.rs` | AI HTTP protocol/configuration and schema serialization |
| `generation/types.rs`, `quality.rs` | Structured contracts, deterministic rules and AI quality review |
| `scoring/http.rs` | Assessment report and retry endpoints |
| `scoring/worker.rs` | Claim, assess and commit scoring work |
| `scoring/rubric.rs` | Evidence validation, grading and competency aggregation |
| `review/mod.rs` | Review route composition and scoped session existence check |
| `review/live.rs`, `assistant.rs` | Live event streams and grounded review chat |
| `review/integrity.rs`, `reports.rs` | Annotations/risk summaries, exports/analytics |
| `infrastructure.rs` | Environment, tracing setup, database connections, expiry transactions |
| `worker.rs` | Run expiry, generation and scoring loops within the API process |
| `observability.rs` | Request correlation and task lifecycle logs |

## Request and task flow

Browser action → `frontend/src/api.ts` → `frontend/src/transport.ts` → HTTP middleware → feature handler → authenticated transaction → database → response.

Media and SSE use the same transport directly, because their response bodies are binary or streamed. JSON tasks finish after parsing; stream tasks finish after stream consumption; recording upload tasks include local draft cleanup.

Generation: invitation/readiness queues preparation → `generation/worker.rs` claims it → `generation/pipeline.rs` validates and publishes → frontend polls status. Scoring: submission → `scoring/worker.rs` claims it → `scoring/rubric.rs` grades evidence → validated results commit. `worker.rs` coordinates these loops without a separate process.

## Where logs appear

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

## Verification

Run backend formatting, Clippy and tests as described in the backend README. The observability test checks request-ID validation/echo and that headers, unknown paths and query secrets do not leak. Existing PostgreSQL integration tests cover access boundaries, lifecycle, deadlines, generation, media, scoring and streaming behavior after the module moves.

In `frontend/`, run `npm test`, `npm run lint` and `npm run build`. Logger tests verify template redaction for known/unknown routes and the production warning default.
