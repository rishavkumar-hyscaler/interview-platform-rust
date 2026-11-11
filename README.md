# AI Interview & Proctoring Platform

Rust/Axum backend, React/TypeScript frontend, PostgreSQL with pgvector.
Interviewers create jobs, invite candidates, and review answers, scores and evidence.
Candidates enter through private invitation links.

## Run locally

Requires Rust stable, Node 22.12+ and PostgreSQL with pgvector.
Preserve existing `.env` files; use the examples only for initial setup.

**Backend**

```bash
cd backend
cp -n .env.example .env
# Configure database credentials and AI settings before continuing.
cargo run -- --migrate
cargo run
```

Provision the restricted `platform_app` database role before migrating. Runtime
connections must not use a superuser or a role with `BYPASSRLS`.
One backend process runs HTTP and all background workers. Default: `127.0.0.1:8080`.
Restart it after Rust changes; Ctrl-C stops the server and workers.

**Frontend — another terminal**

```bash
cd frontend
cp -n .env.example .env
npm ci
npm run dev
```

Default: `http://localhost:5173`. Leave `VITE_API_BASE_URL` empty locally:
Vite proxies `/api` and `/health` to the backend. Use `API_PROXY_TARGET` for another
backend address. If needed, start Vite with `npm run dev -- --port 5174`.
Proctoring models are prepared automatically before dev/build; the first run needs
network access. Restart Vite after environment changes.

## Configuration

See [backend/.env.example](backend/.env.example) and
[frontend/.env.example](frontend/.env.example) for settings.

| Setting | Purpose |
| --- | --- |
| `DATABASE_URL` | Migration connection; also the runtime fallback |
| `RUNTIME_DATABASE_URL` | Restricted application connection |
| `MIGRATION_DATABASE_URL` | Optional migration connection override |
| `FRONTEND_ORIGIN` | Exact allowed frontend origin |
| `ALLOW_REGISTRATION` | Enables interviewer registration when `true` |
| `LLM_BASE_URL`, `LLM_MODEL`, `LLM_API_KEY` | OpenAI-compatible generation/scoring provider |
| `EMBEDDING_MODEL`, `EMBEDDING_API_KEY` | Evidence search and semantic deduplication |
| `INVITATION_ENCRYPTION_KEY` | Stable 64-character hex key for invitations and derived evidence signing |
| `MEDIA_STORAGE_DIR` | Private media directory; defaults to `backend/storage/media` |
| `WEBRTC_ICE_SERVERS` | STUN/TURN configuration for remote live viewing |

Generate the invitation key with `openssl rand -hex 32`. Keep it private and stable.
Back up PostgreSQL, private media and the key together. Never expose media as static
files or put secrets in frontend environment variables. Production requires HTTPS
and an API reverse proxy or a configured `VITE_API_BASE_URL`.

## Application flow

1. **Create job:** save a unique, interviewer-entered Job ID and fixed duration
   (default 10 minutes). A background task prepares the competency blueprint.
2. **Invite candidate:** reuse the saved blueprint and duration; generate a fresh,
   private question set and reference answers. Publish the invitation link only
   after preparation completes. Saved questions survive retries.
3. **Take interview:** fullscreen room, text/code answers, revision-safe autosave,
   resume and server-controlled deadline. Optional audio/video answer recordings
   are available. The timer continues through refreshes and disconnections.
4. **Review:** rubric-based scoring, JD competency profile, integrity evidence,
   session timeline, evidence-grounded chat and JSON export. Evidence embeddings
   are prepared after scoring. Exhausted background tasks support manual retry.

The interviewer manages both setup and review; there is no separate owner role.
Candidates use invitation tokens. Authorized interviewers can retrieve active links
for manual resharing; automatic email delivery is not connected.

## Architecture

```text
backend/src/
  app.rs             Central router and HTTP policies
  main.rs / lib.rs   Startup / application exports
  controllers/       HTTP extraction and response DTO mapping
  services/          Business rules, transactions and workflows
  repositories/      SQLx queries and database projections
  models/ / dto/     Internal types / API contracts
  ai/                Shared Rig integration
  workers/           Generation, scoring, embeddings and expiry loops
  infrastructure.rs  Database pools and transaction-local ownership context
  access.rs          Authentication and scoped access

frontend/src/
  components/        Shared UI components
  features/          Candidate room and interviewer workspace
```

One Cargo package and one executable (`platform-api`). Tokio runs asynchronous
workers inside that process. Durable task state/checkpoints live in PostgreSQL.
Controllers → services → repositories keep HTTP, business rules and SQL separate.
Database RLS scopes rows to the authenticated interviewer; candidate access is
restricted to the invitation's session.

The frontend uses React, TanStack Query, Tailwind and shared shadcn-style components.
MediaPipe runs face/audio checks locally in browser workers. SSE delivers live
updates/chat; WebRTC carries on-demand screen/camera video with authenticated
HTTP signaling. Remote networks may require TURN. Signaling is currently in-memory
and requires one backend instance.

## Database

One development baseline migration in [backend/migrations](backend/migrations/).
Tables: `users`, `auth_sessions`, `jobs`, `question_sets`, `interview_sessions`,
`interview_questions`, `interview_answers`, `audit_events`, `embedding_jobs`,
`evidence_embeddings`.

Sessions reference their private set; answers belong to a session and question.
Observations and attachment metadata use audit records; media bytes stay in private
storage. Rewriting applied migrations is only a development workflow, not a
production upgrade strategy.

## Proctoring and evidence

Camera/microphone permission outcomes are recorded. Enabled devices support these
sustained-condition checks:

| Signal | Threshold |
| --- | --- |
| No face | 2 seconds |
| Multiple faces | 2 seconds |
| Looking away | 4 seconds after calibration |
| Possible background speech | 2 seconds with speech/camera heuristics |

Browser monitoring also records focus, visibility, fullscreen, clipboard and other
supported signals. Copying/pasting the current question is permitted activity.
Flags can capture camera snapshots, short clips and screenshots of the explicitly
shared screen. IndexedDB temporarily buffers pending uploads for recovery.

Event IDs prevent duplicates. Server timestamps, receipt chains, signed metadata
and media hashes detect alteration of received records. Authorized reads verify
integrity. Signatures do not prove that browser telemetry is truthful or complete.
Review priority is separate from answer scores; observations require human review.
Face direction and speech checks do not establish attention, identity or cheating.

Recordings are limited to 60 seconds, 15 MB and 12 per session. Proctor evidence has
separate upload limits. No automatic server-side media retention cleanup is
implemented; operators must manage retention and backups.

## Question-bank analytics

- **Average score:** mean rubric percentage, scaled to 10; `1.5/10` means 15%.
- **Attempts:** recorded question visits; these may differ from scored responses.
- **Average time:** recorded time on a question, including revisits and idle time.
- **Score separation:** Pearson correlation between normalized question score and
  normalized score on the remaining questions, across comparable interviews.

```text
x = question points / question maximum
y = remaining points / remaining maximum
r = Σ((x − mean(x))(y − mean(y)))
    / √(Σ(x − mean(x))² × Σ(y − mean(y))²)
```

Correlation needs at least two valid paired samples and variation in both scores.
There is no 20-attempt gate. Positive values indicate agreement with performance
on the rest of the interview; negative values indicate a reverse pattern. Small
samples are unstable. These statistics do not establish hiring quality or fairness.

## Checks and debugging

Run checks appropriate to the change:

```bash
# backend/
cargo fmt --all -- --check
cargo check
cargo test --locked

# frontend/
npm run lint
npm run build
```

Database integration tests require a dedicated disposable database; some are ignored
by default. Use local AI mocks for routine verification. Representative real-device,
provider and network acceptance testing remains necessary.

Optional Tokio Console:

```bash
# backend/ (install the tokio-console CLI separately)
RUSTFLAGS="--cfg tokio_unstable" cargo run --features tokio-console
# Another terminal:
tokio-console http://127.0.0.1:6669
```

Request/task logs include IDs and durations. Normal `cargo run` disables Console.

## Current limits

- Scoring publishes validated per-question results; true token-by-token scoring
  rationale streaming remains pending. Reviewer chat supports token streaming.
- Adaptive selection chooses from prepared questions; code answers are not executed.
- Reviewer retrieval is session-scoped and citations are validated. Matching quotes
  do not guarantee correct interpretation; AI scoring still needs human oversight.
- Browser monitoring, local heuristics and Web Locks cannot prevent malicious
  clients or reliably detect every display/dev-tools instance.
- No automatic invitation email delivery, distributed WebRTC signaling store or
  automatic media retention cleanup.

See [original_requirement.md](original_requirement.md) for the assignment.
