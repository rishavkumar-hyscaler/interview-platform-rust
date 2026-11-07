# AI Interview & Proctoring Platform — backend

One Rust/Axum backend executable serving HTTP and processing durable PostgreSQL background jobs. The backend supports interviewer/interviewee access, direct job ownership, private invitations, AI question preparation, rubric snapshots, timed interviews, revisioned autosave, resumable navigation, submission, and audit history. The independent React frontend is in `../frontend`.

## Local setup

Requires Rust stable and PostgreSQL with pgvector. Use your existing PostgreSQL at `localhost:5432/interview-platform`. Copy `.env.example` to `.env` only if you do not already have one; preserve your own credentials. The backend automatically loads `.env` when started from `backend/`.

- `DATABASE_URL`: existing connection, used for migrations by default.
- `MIGRATION_DATABASE_URL`: optional migration connection override.
- `RUNTIME_DATABASE_URL`: restricted application connection, used by the backend instead of `DATABASE_URL` when set.

Provision `platform_app` as a non-superuser without BYPASSRLS before applying migrations; the migrations grant its table permissions. Use a private password, not a committed credential. The backend rejects superuser/BYPASSRLS runtime roles. Keep registration enabled only for intentional local/demo onboarding.

```bash
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

One development baseline migration: `migrations/202611040001_initial_schema.sql`.

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

See the [backend layers and logging reference](docs/backend-structure.md) for module responsibilities, request/task flow and log settings.

See [API contract](docs/api.md) for endpoints and limitations and [backend structure](docs/backend-structure.md) for security and release boundaries.

Interviewers sign in with email and password directly. The interviewer account manages interviews and reviews evidence. Candidates use invitation links. There is no owner account, workspace-ID entry, or organization-creation form. Ownership is derived directly from the authenticated interviewer.

## Saved invitation links

`INVITATION_ENCRYPTION_KEY` in `.env` must contain a stable 64-character hex key (`openssl rand -hex 32`). The local configuration has been provisioned privately. Keep this key backed up separately from PostgreSQL; do not change it without a key migration. New invitation secrets are stored encrypted and can be retrieved by the authorized recruiter from session details. Existing hash-only invitations require an explicit replacement before acceptance. Retrieval and replacement are audited; expiry is preserved. This enables manual resharing, not automatic email delivery.

After changing backend code, stop the running cargo process with Ctrl-C and start `cargo run` again. Rust is not hot-reloaded by Vite. The timed room uses the existing schema; no additional migration is required.

## Private answer recordings

Candidates can attach short audio/video recordings. `MEDIA_STORAGE_DIR` defaults to `backend/storage/media` and is gitignored; back it up alongside the database. Clips are served only through authenticated APIs, with content-hash checks. Browser capture stops at 60 seconds, server uploads are limited to 15 MB and 12 recordings per interview. Completed unsent clips recover through IndexedDB. Written answers drive AI scoring; media-only answers remain for human review. No migration is needed.

## Interviewer-entered Job IDs

Job creation requires `job_code`, shown as Job ID in the UI (for example `BE-2026-001`). Codes are trimmed, normalized to uppercase and unique within the interviewer account. Use 1–64 ASCII letters/digits/hyphens/underscores, starting with a letter or digit. The internal UUID remains the foreign key used by invitation and question-bank routes. Fresh databases use the single development baseline via `cargo run -- --migrate`.

## Camera/audio proctoring

See [media proctoring setup, flow and limits](docs/media-proctoring.md) for local models, candidate controls, evidence storage and verification. No additional migrations are needed.

## Questions prepared when a job is created

Job creation queues the selected 1–5 reusable question sets (default 1) with reference answers, before invitations. Choose the preparation duration in the job form; inspect **Question sets** on the job card. Invitations reuse a completed matching set; assignment does not generate replacements. See [AI workflows](docs/ai.md) for generation, retries and assignment.

Question preparation completes one question at a time: generate a complete prompt/reference/rubric, validate and deduplicate it, independently review it, then save a durable checkpoint before proceeding. Retries resume from the first unsaved question. Candidates can start only after the set is complete; interviewers can inspect saved progress and rejection findings. Each question has a five-minute default preparation budget. See [sequential generation and recovery](docs/ai.md). Existing schema and completed interviews are preserved.

## Persistent post-scoring RAG

After `assessment.completed`, a separate durable embedding job indexes final answers, shared question/rubric text, assessment reasoning and proctoring/annotation/attachment metadata into pgvector. Review chat embeds only the interviewer’s query. The UI shows queued/running/ready/failed state and offers retry; scores remain complete if indexing fails. See [RAG indexing and recovery](docs/rag.md).

For an existing database using the immediately preceding single baseline, run `cargo run -- --upgrade-rag` once from `backend/`, then restart `cargo run`. This adds only the two RAG tables, indexes and queue triggers and updates the verified development baseline checksum; it preserves existing rows. Fresh databases use `cargo run -- --migrate`. There is still only one migration file.

Backend code organization: [controller/service/repository structure](docs/backend-structure.md).

## Documentation

- [Backend layers, ownership and logging](docs/backend-structure.md)
- [API contract](docs/api.md)
- [AI generation, scoring and Rig](docs/ai.md)
- [Evidence indexing and RAG](docs/rag.md)
- [Media proctoring](docs/media-proctoring.md)
- [Requirement coverage and remaining work](docs/requirement-status.md)
