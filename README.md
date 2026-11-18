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
# Override DATABASE_URL with the database owner's URL for migrations only.
DATABASE_URL='postgres://postgres:replace_me@localhost:5432/interview-platform' cargo run -- --migrate
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

## Run on local Kubernetes

Use the [Minikube guide](deploy/local/README.md) to run the backend in local
Kubernetes with persistent evidence storage, a migration Job and health probes.
The existing AWS deployment configuration remains available for future use.

## Deploy frontend to Vercel

Import the Git repository in Vercel and set **Root Directory** to `frontend`.
The included `frontend/vercel.json` configures installation, build and output.
Model downloads run automatically during the build and require network access.

Set `VITE_API_BASE_URL` to the public HTTPS backend origin, without `/api` or a
trailing slash. Do not add database credentials or AI keys to Vercel's frontend
variables. Redeploy after changing the API URL because Vite embeds it at build time.

The frontend can be published before the backend, but sign-in and interview flows
will remain unavailable until a reachable API is configured. Vite's local API proxy
does not run on Vercel. Once the backend is available, allow the exact frontend
origin through its `FRONTEND_ORIGIN` setting.

## Deploy backend to AWS

See [DEPLOYMENT.md](DEPLOYMENT.md) for the complete deployment revision guide,
resource inventory, IAM responsibilities, configuration and troubleshooting.

Start with the [single EC2 guide](deploy/aws/ec2/README.md): Docker Compose runs
the backend and Caddy HTTPS proxy, using Neon and the existing Vercel frontend.
GitHub Actions can deploy to EC2 through Systems Manager after one-time AWS setup.

```text
GitHub Actions
  │ Access keys + DeployInterviewPlatformEC2 policy
  ▼
Systems Manager sends deployment command
  ▼
EC2 instance
  │ interview-platform-ec2-role
  ▼
Reads Parameter Store → pulls image → runs backend
```

The deployment user's policy lets GitHub send commands to the designated instance.
The EC2 role lets the server communicate with Systems Manager and read its private
configuration from Parameter Store. Access keys stay in GitHub Secrets.

The EKS deployment, Neon setup, GitHub Actions OIDC configuration,
TLS, persistent media and rollback steps are in [the deployment guide](deploy/aws/README.md).
Existing backend CI must pass before an immutable container image is published.
EKS deployment stays disabled until `BACKEND_DEPLOY_ENABLED=true` is configured
in GitHub. For EC2, also set `BACKEND_DEPLOY_TARGET=ec2` as described in its guide.

## Configuration

See [backend/.env.example](backend/.env.example) and
[frontend/.env.example](frontend/.env.example) for settings.

| Setting | Purpose |
| --- | --- |
| `DATABASE_URL` | Only database setting; use restricted credentials for the API, owner credentials for migrations |
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
   Each set's seed gives every question its own framing, phrasing voice, setting
   and numeric parameters (`services/generation/variation.rs`), so candidates for
   the same JD get paraphrased, re-parameterized tasks. Scenario and code prompts
   must use at least one assigned value, and exact and semantic duplicates of
   earlier JD questions are still rejected. The applied variation is audited.
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

The app is a **modular monolith**: one React frontend, one Rust backend process,
PostgreSQL with pgvector, private media storage and an external AI provider.

```mermaid
flowchart TB
    subgraph Browser
        C[Candidate room]
        I[Interviewer workspace]
        P[Local face/audio analysis]
        Q[IndexedDB upload queue]
        C --> P --> Q
    end
    subgraph Backend[Rust backend]
        API[Axum router and middleware]
        CTRL[Controllers and DTOs]
        SVC[Services]
        REPO[Repositories / SQLx]
        W[Tokio background workers]
        AI[Rig AI integration]
        API --> CTRL --> SVC --> REPO
        W --> SVC --> AI
    end
    C -->|HTTP requests| API
    I -->|HTTP requests| API
    Q -->|Events and media uploads| API
    API -->|SSE updates and chat| I
    C <-->|WebRTC screen/camera video| I
    REPO --> DB[(PostgreSQL + pgvector)]
    SVC --> MEDIA[Private media storage]
    AI --> LLM[Configured AI provider]
```

### Backend layers

```text
Request → router/middleware → controller → service → repository → PostgreSQL
```

| File/folder under `backend/src/` | Responsibility |
| --- | --- |
| `main.rs` / `lib.rs` | Startup, shutdown / application exports |
| `app.rs` | Central router composition, HTTP policies and error normalization |
| `controllers/` | HTTP extraction, authentication and response DTO mapping |
| `dto/` | Explicit API request and response contracts |
| `services/` | Business rules, transactions and workflows |
| `repositories/` | Parameterized SQLx queries and database projections |
| `models/` | Internal data structures |
| `ai/` | Rig adapters for structured output, streaming, embeddings and transport |
| `workers/` | Generation, scoring, embeddings and expiry loops |
| `infrastructure.rs` | Database pools, ownership-scoped transactions and tracing |
| `access.rs` | Authentication and scoped authorization |
| `invitations.rs` | Invitation cryptography and evidence-signing key derivation |
| `openapi.rs` | API documentation, enabled in debug builds |

One Cargo package contains a binary crate (`main.rs`) and library crate (`lib.rs`),
with one executable, `platform-api`. Service folders and workers are modules, not
separate executables. Controllers map internal results to API response DTOs.

The SQLx pool allows up to 10 connections. Cloning `PgPool` shares that pool rather
than creating another one. Database RLS scopes rows using transaction-local
interviewer context; candidate access is restricted to the invitation's session.

### Frontend modules

| File/folder under `frontend/src/` | Responsibility |
| --- | --- |
| `main.tsx` | Entry point, authentication and candidate/interviewer experiences |
| `features/interview/` | Invitation access, devices, timer, questions, autosave, fullscreen and recordings |
| `features/interview/proctoring/` | Browser signals, local inference, evidence capture and upload recovery |
| `features/workspace/` | Jobs, invitations, results, analytics, audit, assessment and reviewer chat |
| `features/live-screen/` | Candidate WebRTC sender, interviewer viewer and signaling |
| `components/ui/` | Shared buttons, cards, inputs, pagination, toolbars and accordions |
| `queryClient.ts` | TanStack Query server-data cache configuration |
| `api.ts` / `transport.ts` | Requests, authentication headers, cancellation, timeouts and logging |
| `session.ts` | Frontend session handling |

React renders the UI; feature API modules communicate with the backend. Tailwind
and shared shadcn-style components provide reusable styling. MediaPipe performs
face/audio checks locally in browser workers.

### Background processing

Four asynchronous Tokio loops run alongside HTTP in the same process:

| Worker | Work |
| --- | --- |
| Generation | Prepare job blueprints and candidate questions/reference answers |
| Scoring | Grade submitted answers and aggregate competency results |
| Embeddings | Build searchable session evidence vectors |
| Expiry | Finalize interviews whose deadlines have passed |

Task state and checkpoints live in PostgreSQL. Workers share the pool and shutdown
signal; they do not create an operating-system thread for every candidate.

### Main pipelines

**Preparation:** create job → save fixed duration → background blueprint preparation
→ invite candidate → generate and validate fresh private questions incrementally
→ publish invitation link when preparation completes.

**Interview:** validate invitation → device checks and fullscreen → server-controlled
timer → fetch question → revision-safe autosave → navigation/adaptive selection
→ manual submission or deadline expiry.

**Scoring and retrieval:** submitted interview → rubric grading (streamed) → validate criterion
IDs, points and answer quotes → competency profile → evidence text chunks
→ embeddings → pgvector. Reviewer questions retrieve session-scoped sources,
stream an AI reply, validate citations and save the accepted conversation.
Image pixels are not embedded; event and attachment metadata provide textual evidence.
While each answer is graded, `GET /api/v1/sessions/{id}/assessment/stream` (SSE)
sends the criterion rationale as the model writes it, marked unverified, then a
`scored` event once that grade passes validation. The feed lives in the worker's
process; the stored assessment stays authoritative.

**Proctoring:** browser events/local inference → sustained-condition detection
→ observation and relevant snapshot/clip → IndexedDB queue → backend upload
→ scoped event/media storage → signed receipt records and media hashes
→ interviewer evidence view → logged reviewer annotations.

### Communication and storage boundaries

| Mechanism | Purpose |
| --- | --- |
| HTTP | API calls, autosave, uploads and WebRTC signaling |
| SSE | Live reviewer updates and streamed chat |
| WebRTC | On-demand candidate screen/camera video; no microphone audio in live viewing |
| IndexedDB | Temporary browser-side evidence upload recovery |
| PostgreSQL / pgvector | Application records, durable tasks and evidence vectors |
| Private media directory | Screenshot and recording bytes; database stores their references |

WebRTC reuses the candidate's screen/camera tracks. Authenticated HTTP polling
exchanges offers/answers; video travels between peers or through TURN when needed,
not through the application API. Signaling rooms are in memory, require one backend
instance and are lost on restart.

Security boundaries include authentication, ownership checks, RLS, parameterized
queries, request validation and authorized media access. AI prompts treat supplied
content as untrusted data; scores and citations are validated server-side. Signed
receipts detect alteration of received records, not dishonest or omitted browser
telemetry. Request/task logs provide traceable IDs and durations.

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
The integrity review shows a one-line AI summary that separates reviewer-confirmed,
suspected and routine observations. The server rejects it unless it is one line and
quotes the session's observation sources exactly. Summaries are cached in the audit
log (`integrity.summarized`) per evidence fingerprint and regenerate when observations,
media or annotations change. If the AI is unavailable, a summary built from counts is
shown instead and labelled as such. Exports include the summary that matches the
current evidence, when one exists.
Face direction and speech checks do not establish attention, identity or cheating.

Recordings are limited to 60 seconds, 15 MB and 12 per session. Proctor evidence has
separate upload limits. No automatic server-side media retention cleanup is
implemented; operators must manage retention and backups.

## Bias guardrails

Scores and flags must rest on job-relevant evidence only. Identity signals such as
name, email, accent, voice, appearance and protected characteristics are kept out
of AI inputs where possible, and models are told to ignore them where not.

| Stage | Data the AI receives | Guardrail |
| --- | --- | --- |
| Blueprint | Job description | Extract only job-relevant competencies, each citing an exact JD quote; ignore identities and protected characteristics (`services/generation/quality.rs`) |
| Question review | JD, blueprint, draft question | Reject prompts that require protected characteristics or unstated facts |
| Answer scoring | Question prompt, stored rubric, answer text only | No name, email, media or proctoring data is sent; the prompt says to ignore identity, names, accent and appearance (`services/scoring/rubric.rs`) |
| Score validation | Model output | The server accepts a score only when criterion IDs match the rubric, points stay in range and awarded points quote the answer exactly |
| Reviewer chat | Session questions, rubrics, answers, assessment, proctoring events, annotations | No candidate name or email is indexed; the prompt forbids identity-based assessment and reading face geometry as attention, emotion or identity (`services/review_chat_service.rs`) |

Proctoring checks avoid identity inference:

- Face and audio checks run in the browser and report only face counts, head/eye
  geometry and speech heuristics. There is no face recognition, identity matching,
  speaker identification or emotion detection.
- Audio/video recordings are not AI-scored. They need a text transcript for AI
  scoring; otherwise a human reviews them.
- Default integrity weights give zero risk to signals that often reflect disability,
  environment or device rather than misconduct: denied camera or microphone,
  looking away and detected speech. Operators can change weights with
  `INTEGRITY_WEIGHTS`.
- Integrity risk points stay separate from answer scores, and every observation
  needs human review. Reviewer overrides are logged.

**Limits:** these guardrails come from input minimization, prompt instructions and
server-side validation. They cannot stop a model reacting to identity cues inside an
answer, such as a candidate's self-introduction or writing style. No
disparate-impact audit is performed. Question-bank analytics measure score
separation, not fairness. Hiring decisions stay with human reviewers.

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
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked

# frontend/
npm run lint
npm run format:check   # npm run format to fix
npm run build
```

Regular tests need no PostgreSQL. For database integration tests, use a migrated
disposable database and the restricted `platform_app` role. They insert and delete
data; do not point them at the main development or production database.

```bash
# backend/ — use your disposable database's URL and a test-only invitation key.
DATABASE_URL='postgres://platform_app:replace_me@localhost:5432/interview-test' \
ALLOW_REGISTRATION=true \
INVITATION_ENCRYPTION_KEY=0000000000000000000000000000000000000000000000000000000000000000 \
cargo test --tests --locked -- --ignored --test-threads=1
```

GitHub Actions supplies `DATABASE_URL` for its temporary PostgreSQL container;
it never uses your local `.env` or Neon database. AI tests use local mocks.
Representative provider, real-device and network acceptance checks remain necessary.

Optional Tokio Console:

```bash
# backend/ (install the tokio-console CLI separately)
RUSTFLAGS="--cfg tokio_unstable" cargo run --features tokio-console
# Another terminal:
tokio-console http://127.0.0.1:6669
```

Request/task logs include IDs and durations. Normal `cargo run` disables Console.

## Current limits

- Live scoring rationale is in-process: with several API replicas, a reviewer on a
  different replica than the scoring worker sees only the final assessment.
- Adaptive selection chooses from prepared questions; code answers are not executed.
- Reviewer retrieval is session-scoped and citations are validated. Matching quotes
  do not guarantee correct interpretation; AI scoring still needs human oversight.
- Browser monitoring, local heuristics and Web Locks cannot prevent malicious
  clients or reliably detect every display/dev-tools instance.
- No automatic invitation email delivery, distributed WebRTC signaling store or
  automatic media retention cleanup.

See [original_requirement.md](original_requirement.md) for the assignment.
