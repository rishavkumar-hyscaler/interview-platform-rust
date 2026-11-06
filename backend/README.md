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

The same process starts the expiry job loop automatically; no second backend command is needed. Ctrl-C or SIGTERM stops both HTTP and background processing gracefully.

The API defaults to `127.0.0.1:8080`. Set `FRONTEND_ORIGIN` to the frontend's exact origin. For a preview on an occupied machine, `FRONTEND_ORIGIN=http://localhost:5174 cargo run` overrides `.env` without editing it.

An optional Compose database remains available via `docker compose -f compose.yaml up -d db` on port 55432. Its database is named `interview`; update connection URLs explicitly if choosing it instead of your existing PostgreSQL.

## Schema

One development baseline migration: `migrations/202611040001_initial_schema.sql`.

Eight application tables: `users`, `auth_sessions`, `jobs`, `question_sets`, `interview_sessions`, `interview_questions`, `interview_answers`, and `audit_events`. Users are interviewers or interviewees. Jobs own reusable question sets; generation attempts, retries and leases live only in `question_sets`. Sessions reference `question_set_id`; questions reference `set_id`. Answers are scoped by both session and question. Per-candidate generation, question copies, organizations and memberships are absent.

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
  Cargo.toml
  src/
    main.rs             # Startup, HTTP server, shared shutdown
    lib.rs              # Module inventory and public entry points
    app.rs              # Router composition and HTTP policies
    auth.rs, access.rs  # Authentication and scoped database access
    jobs.rs             # Job creation and listing
    interviews/         # Invitations, staff views, candidate lifecycle
    generation/         # Scheduling, worker, validated question pipeline
    scoring/            # HTTP endpoints, worker, rubric rules
    review/             # Live streams, assistant, integrity and reports
    observability.rs    # Request IDs and task lifecycle logs
    infrastructure.rs   # Configuration, database, durable expiry transaction
    worker.rs           # In-process background job loop
  tests/                # Integration tests
  migrations/           # Single development baseline
```

`cargo build --release` produces `target/release/platform-api`. `cargo run` starts the entire backend. `cargo run -- --migrate` applies migrations and exits.

See the [code guide and logging reference](docs/code-guide.md) for module responsibilities, request/task flow and log settings.

See [API contract](docs/api.md) for endpoints and limitations and [architecture](docs/architecture.md) for security and release boundaries.

Interviewers sign in with email and password directly. The interviewer account manages interviews and reviews evidence. Candidates use invitation links. There is no owner account, workspace-ID entry, or organization-creation form. Ownership is derived directly from the authenticated interviewer.

## Saved invitation links

`INVITATION_ENCRYPTION_KEY` in `.env` must contain a stable 64-character hex key (`openssl rand -hex 32`). The local configuration has been provisioned privately. Keep this key backed up separately from PostgreSQL; do not change it without a key migration. New invitation secrets are stored encrypted and can be retrieved by the authorized recruiter from session details. Existing hash-only invitations require an explicit replacement before acceptance. Retrieval and replacement are audited; expiry is preserved. This enables manual resharing, not automatic email delivery.

After changing backend code, stop the running cargo process with Ctrl-C and start `cargo run` again. Rust is not hot-reloaded by Vite. The timed room uses the existing schema; no additional migration is required.

## Private answer recordings

Candidates can attach short audio/video recordings. `MEDIA_STORAGE_DIR` defaults to `backend/storage/media` and is gitignored; back it up alongside the database. Clips are served only through authenticated APIs, with content-hash checks. Browser capture stops at 60 seconds, server uploads are limited to 15 MB and 12 recordings per interview. Completed unsent clips recover through IndexedDB. Written answers drive AI scoring; media-only answers remain for human review. No migration is needed.

## Interviewer-entered Job IDs

Job creation requires `job_code`, shown as Job ID in the UI (for example `BE-2026-001`). Codes are trimmed, normalized to uppercase and unique within the interviewer account. Use 1–64 ASCII letters/digits/hyphens/underscores, starting with a letter or digit. The internal UUID remains the foreign key used by invitation and question-bank routes. The additive migration assigns existing records `LEGACY-<UUID without hyphens>` without deleting data. Apply using `cargo run -- --migrate`, then restart the backend.

## Camera/audio proctoring

See [media proctoring setup, flow and limits](docs/media-proctoring.md) for local models, candidate controls, evidence storage and verification. No additional migrations are needed.

## Questions prepared when a job is created

Job creation queues three unique question sets with reference answers, before invitations. Choose the preparation duration in the job form; inspect **Question sets** on the job card. Invitations reserve an unused matching set and queue its replacement. See [job question-set flow](docs/job-question-sets.md) for fallback generation, retries, migration and API details.

Question preparation completes one question at a time: validate/deduplicate the prompt, generate/review its reference, then save a durable checkpoint before proceeding. Retries resume from the first unsaved question. Candidates can start only after the set is complete; interviewers can inspect saved progress and rejection findings. Each question has a five-minute default preparation budget. See [sequential generation and recovery](docs/generation-timing.md). Existing schema and completed interviews are preserved.
