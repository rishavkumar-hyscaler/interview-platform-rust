# AI Interview & Proctoring Platform — backend

One Rust/Axum backend executable serving HTTP and processing durable PostgreSQL background jobs. Phase 1 includes authentication, tenant isolation, jobs with versioned JDs, candidate invitations, guarded session transitions, audit history, and cursor pagination. The independent React frontend is in `../frontend`.

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

## Migration policy

Keep the existing foundation migration and one cohesive Phase 1 domain migration. Add future migrations only for meaningful schema changes. Never rewrite an already applied migration: SQLx verifies checksums and existing installations must upgrade safely. No automatic database reset is performed.

## Verification

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
ALLOW_REGISTRATION=true cargo test --tests --locked -- --ignored --test-threads=1
```

Integration tests load `.env`, prefer `RUNTIME_DATABASE_URL`, and add uniquely named test accounts/jobs/sessions. Use a dedicated database when testing on shared infrastructure. Tests cover authentication, revocation, tenant isolation, recruiter/candidate access boundaries, idempotent creation, cursor pagination, private candidate capabilities, guarded transitions, immutable audit records, composite tenant foreign keys, and concurrent expiry workers.

## Backend layout

One `Cargo.toml`, one `Cargo.lock`, and one executable (`platform-api`):

```text
backend/
  Cargo.toml
  src/
    main.rs             # Startup, HTTP server, shared shutdown
    lib.rs              # Router and authentication
    interviews.rs       # Interview APIs and authorization
    infrastructure.rs   # Configuration, database, durable expiry transaction
    worker.rs           # In-process background job loop
  tests/                # Integration tests
  migrations/           # Versioned schema upgrades; preserve applied checksums
```

`cargo build --release` produces `target/release/platform-api`. `cargo run` starts the entire backend. `cargo run -- --migrate` applies migrations and exits.

See [API contract](docs/api.md) for endpoints and limitations and [architecture](docs/architecture.md) for security and release boundaries.

Recruiters sign in with email and password directly. The same account manages interviews and reviews evidence. Candidates use invitation links. There is no owner account, workspace-ID entry, or organization-creation form. Internal scope IDs preserve record isolation and existing data.

## Saved invitation links

`INVITATION_ENCRYPTION_KEY` in `.env` must contain a stable 64-character hex key (`openssl rand -hex 32`). The local configuration has been provisioned privately. Keep this key backed up separately from PostgreSQL; do not change it without a key migration. New invitation secrets are stored encrypted and can be retrieved by the authorized recruiter from session details. Existing hash-only invitations require an explicit replacement before acceptance. Retrieval and replacement are audited; expiry is preserved. This enables manual resharing, not automatic email delivery.
