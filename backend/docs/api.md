# API contract

There are two user types: interviewer and interviewee. There are no organization IDs, memberships, or workspace-selection endpoints. Errors use `{error, code}`. Success responses disable caching.

## Authentication

- `POST /api/auth/register`: `{email,password}` creates an interviewer, when registration is enabled.
- `POST /api/auth/login`: `{email,password}` signs in an interviewer.
- Both return `{token,user_id,role:"interviewer"}`.
- `GET /api/me` validates an interviewer bearer token and returns `{user_id,role}`.
- `POST /api/auth/logout` revokes that login token.

Interviewees use private invitation capabilities and do not need a password. Invitation access cannot authenticate to interviewer endpoints.

## Interviewer endpoints

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

## Invitations

Creation returns `{session,invitation_token}`. New invitation tokens are hashed for validation and encrypted for recovery in `auth_sessions`. The UI builds `/#invite=<token>`. The fragment stays in the URL so refresh and same-tab navigation retain interviewee access. Opening the link automatically validates it.

Recovery is interviewer-authorized and audited, never extends expiry, and cannot share expired/submitted interviews. Old hash-only links cannot be reversed: replacing an unaccepted legacy link invalidates it explicitly. Accepted legacy links are not replaced. Retrieval/replacement never sends email automatically. Keep `INVITATION_ENCRYPTION_KEY` privately backed up.

## Interviewee endpoints

- `GET /api/v1/candidate/session`: the assigned session only, without rubrics or other participants' data.
- `POST /api/v1/candidate/session/ready`: accepts the invitation.
- `POST /api/v1/candidate/session/start`: requires prepared questions and sets the deadline once.
- `POST /api/v1/candidate/session/submit`: submits an in-progress interview.

Transitions are guarded and retries do not reset deadlines or duplicate audit events. `GET /api/v1/candidate/room?position=N` returns exactly one question and its saved answer/revision, never the rubric. Omitting position restores the latest served question from the audit trail. Reading before start returns 409; after submission it returns the receipt with no editable question.

`POST /api/v1/candidate/answers/{question_id}` accepts `{content,revision}` (revision 0 for a new answer). Content is bounded to 48000 UTF-8 bytes. Changed answers advance their revision; identical retries retain it and produce no duplicate save audit event. Conflicting edits return 409. The question must belong to the capability's assigned session.

All answer writes, navigation, and submission serialize on the interview row. Server clock time is checked after acquiring the lock. At deadline, an in-progress session becomes submitted at its persisted deadline; unstarted invitations expire. Saved answers remain available to the interviewer, while late writes are rejected. Refresh/reconnect never reset time. Back/next navigation is allowed throughout the overall interview; per-question elapsed and suggested time are shown without separate per-question lockouts.

Scoring, adaptive difficulty, camera/audio answers, fullscreen enforcement, and browser proctoring remain later work.

## Question preparation

Configuration uses `LLM_PROVIDER=openai`, `LLM_MODEL=gpt-5-mini`, `LLM_API_KEY`, `EMBEDDING_MODEL=text-embedding-3-small`, and `EMBEDDING_API_KEY`. `LLM_BASE_URL` defaults to `https://api.openai.com/v1`.

When AI is configured, creating an invitation automatically queues question preparation in the same transaction. Opening or accepting an older, unprepared invitation also queues it once. Repeated candidate polls do not duplicate jobs, regenerate prepared questions, or reset failed attempts. Failed generation still requires an explicit interviewer retry. With missing configuration, the candidate sees a setup message rather than a promise of progress.

A preparation request moves the session's generation status through pending/running/completed/failed. A five-minute lease, renewed every 30 seconds during preparation, and a fencing token support recovery. Failures retry up to three attempts; explicit retry resets the attempt budget. Schema, evidence quotations, rubric totals, and embedding output are validated before publishing. Exact fingerprints and pgvector cosine similarity reject repeats across sessions for the same job. Each question stores its rubric snapshot and generation provenance.

Provider request formats follow the official [Structured Outputs](https://developers.openai.com/api/docs/guides/structured-outputs?api-mode=responses) and [embeddings](https://developers.openai.com/api/reference/resources/embeddings/methods/create) documentation. No raw provider response or API secret is written to audit records.

### Browser observation capture
`POST /api/v1/candidate/events` accepts 1–40 `{id: UUID, kind, client_time: epoch_ms, count?: number}` observations using the invitation bearer capability. Only active interviews can write. Server timestamps are authoritative receipt times; client times and all browser observations are unverified. UUID retries are deduplicated in the immutable audit log. The session row serializes writes and caps storage at 10,000 observations. Paste events contain character counts, never clipboard text. Existing interviewer audit pagination displays the observation evidence.

The browser queues unsent observations on this device and retries after reconnect. This is best-effort capture, not tamper-proof telemetry: client scripts/storage can be modified and cross-tab delivery is not guaranteed. Fullscreen is requested through an explicit browser gesture; denial keeps monitoring active. Device permission denial is recorded when the device button is used. Server receipt signatures, browser single-tab locking and configurable integrity risk points are implemented. Optional recorded media answers are implemented separately; optional face/voice analysis is not included. Dev-tools detection is a window-size heuristic and can be wrong; multiple-display detection depends on browser support.

### Single active browser tab
The candidate view acquires an exclusive Web Lock named with a SHA-256 invitation digest before mounting any candidate API calls, editor, autosave or device capture. Another tab waits without mounting these features and automatically enters after the owner closes. Browser-managed release handles refresh/crashes without stale local-storage leases. Unsupported browsers show an explicit compatibility message. This guards one origin/profile only; localhost vs 127.0.0.1, separate profiles, browsers and devices are independent. It is not a server authorization boundary. The server continues to enforce answer revisions and deadlines. Lock waiting does not pause time.

### Rubric assessment
The existing background worker automatically claims submitted interviews, writes an `assessment.started` audit event, and grades all saved answers. At most three attempts are made with a 15-minute claim timeout and fencing against stale workers. Results are stored immutably in `assessment.completed`; no new table is required. Empty answers receive zero without an AI call. Every awarded criterion score must quote an exact substring of the candidate answer and stay within the stored rubric maximum. Candidate content is untrusted data; no tools are exposed to the scoring model. Identity signals are excluded by instruction, and human review remains required.

`GET /api/v1/sessions/{session}/assessment` is interviewer-only and scoped by row-level security. It returns the scoring state, attempts, validated criteria and browser observations with event IDs and server receipt timestamps. The results view polls while waiting and exports its report as JSON. The worker loads AI configuration on startup, so restart after configuration changes. Each validated question result is emitted to the live audit stream while scoring proceeds, with a provisional marker until the whole assessment completes. A competency-weighted profile and evidence recommendation accompany the final result. Explicit retry after expired/exhausted attempts is available; reviewer RAG chat streams separately.


### Review, adaptive selection and operations
- `GET /api/v1/sessions/{id}/live?after=<event UUID>`: authenticated SSE audit stream, ordered server timestamp/UUID cursor, heartbeat, bounded replay and periodic credential checks. Browser reconnects with the last event ID. Scoring progress is validated per question; provisional progress is not the final assessment.
- `POST /api/v1/sessions/{id}/review`: `{question,conversation:UUID,page:0}`. Embeds 40-source evidence pages (3,000-character chunks), ranks them with pgvector cosine distance, and retrieves eight sources. Streams actual Responses API text deltas over SSE; partial text is labelled unverified and is discarded on failure. The final `complete` event contains validated exact citations and source text. Up to six prior messages from this interviewer/session/conversation supply context, not evidence. Pages are bounded, so the assistant must acknowledge incomplete evidence; use Next evidence page to inspect more sources. No tools or cross-session retrieval are exposed.
- `POST /api/v1/candidate/recommendation`: `{question_id}`. Saves remain explicit through the existing answer API. Scores the current answer provisionally and chooses an unserved prepared question nearest the target difficulty: >=8/10 harder, <=4/10 easier, otherwise unchanged. Revision checks reject results based on a changed answer. Only the chosen position reaches the candidate; rubric scores remain private. Final scoring re-evaluates submitted answers.
- `GET /api/v1/sessions/{id}/integrity`: configurable weighted risk points and exact event evidence, latest reviewer annotation and signature-chain verification. Set optional `INTEGRITY_WEIGHTS` JSON (known keys, integers 0–20) to adjust review priorities. A score is not a probability or cheating verdict. Default weights are returned in each report.
- `POST /api/v1/sessions/{id}/annotations`: `{event_id,decision,note}`. Decisions: `dismissed`, `suspected`, `confirmed_activity`. Notes and actor/time are appended, never overwritten. Confirmed activity does not mean confirmed cheating.
- `POST /api/v1/sessions/{id}/assessment/retry`: starts a new three-attempt budget only after any current 15-minute claim has expired. Original attempt history remains intact.
- `GET /api/v1/sessions/{id}/report`: full JSON hiring record with prompts, rubric snapshots, saved answers, scores, integrity evidence, annotations and audit IDs. Export is logged. Treat exports as private candidate records.
- `GET /api/v1/jobs/{id}/analytics`: descriptive mean, spread and item–total correlation grouped by competency/type/difficulty. The UI withholds correlation interpretation below 20 observations. Unique single-use questions do not have enough repeat observations for statistically validated individual discrimination; correlation is exploratory.
- `GET /api/v1/metrics`: interviewer-scoped generation/scoring p50 and p99, session counts, observation counts/rates and scoring failures, computed from immutable server timestamps. Existing request traces plus assessment duration logs omit candidate answers and API keys.

Proctor events carry monotonic server sequence, previous signature, authoritative receipt time and a domain-separated HMAC using a key derived from the stable invitation key. Reusing an event ID for changed content returns 409. Legacy events remain visibly unsigned. These signatures detect changes to received records; a browser controlled by the candidate can still suppress or fabricate telemetry before receipt. Never label browser observations as trusted solely because the server signed receipt.

Streaming implementation follows [official OpenAI documentation](https://developers.openai.com/api/docs/guides/streaming-responses). Partial output is not citation-verified until completion.


### Optional recorded answers
- `POST /api/v1/candidate/questions/{question}/recordings/{recording UUID}`: raw audio/video bytes with `Content-Type` (`audio/webm`, `video/webm`, `audio/ogg`, `audio/mp4`, `video/mp4`; codec parameters allowed). Container header and size are checked. Up to 15 MB per clip, 12 clips per interview. New uploads require the assigned question and an active interview both before and after file persistence. Identical retries return the same audit reference even after submission; changed content under the same ID returns 409.
- `GET /api/v1/candidate/recordings` and `GET /api/v1/candidate/recordings/{id}`: private own-session metadata and bytes, authenticated with the invitation capability.
- `GET /api/v1/sessions/{session}/recordings` and `GET /api/v1/sessions/{session}/recordings/{id}`: interviewer-only metadata and playback. Files never have public URLs or URL credentials. Playback fetches an authenticated blob and verifies the stored SHA-256 before delivery.

Browser MediaRecorder stops at 60 seconds (or a conservative size threshold), stores a completed blob in IndexedDB, then attempts upload. Interrupted uploads can retry after reconnect or reopening the tab; unsent clips can be downloaded or explicitly discarded. Pending recordings block manual navigation/finish. The overall deadline continues and can end a recording; a late clip remains local rather than silently entering the submitted assessment. The submitted receipt offers local downloads for clips that may not have reached the server. An unfinished recording lost in a browser crash is not guaranteed recoverable; stopped recordings are journalled before upload.

Storage is `MEDIA_STORAGE_DIR`, default `backend/storage/media`, outside static frontend assets. Files use owner/session/recording UUID paths and Unix mode 0600; lightweight references, content hashes and timestamps use existing append-only `answer.recorded` audit events. Cancelled uncommitted files can be repaired on retry under a session lock. Back up the private media directory with the database. There is no new table or migration. JSON report exports carry references, not embedded media files.

Recordings count toward answered-question progress. AI scoring uses written answers only; a media-only question is marked `media_requires_human_review` with a null score and excluded from numeric AI totals/weighted competency coverage. Reviewer chat can retrieve attachment metadata, but cannot interpret the recording contents without a written transcript.


### Generation v2: worked reference answers and publication gates
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

### Human-readable Job IDs
`POST /api/v1/jobs` requires `job_code`. The API trims and uppercases it; codes are 1–64 ASCII letters, digits, hyphens or underscores and must start with a letter or digit. The database enforces uniqueness per interviewer, so case/whitespace-equivalent duplicate creation (including simultaneous requests) returns 409. Identical retries using the same request key remain idempotent; changing the Job ID under an existing key returns 409. Another interviewer may use the same code independently. Job lists, session lists/details, candidate session data and JSON reports include `job_code`; internal `id`/`job_id` UUIDs and route parameters are unchanged. Existing jobs receive deterministic `LEGACY-…` codes through one additive migration.


## Media proctoring evidence

`POST /api/v1/candidate/events` additionally accepts the media observation kinds and bounded optional `measurements` object documented in [media proctoring](media-proctoring.md). Duplicate event IDs must retain the same measurements.

- `POST /api/v1/candidate/events/{event}/evidence/{asset}`: binary JPEG/audio/video body, candidate bearer token; binds an attachment to a recent allowed event. Retry with the same asset UUID and identical bytes.
- `GET /api/v1/sessions/{session}/evidence/{asset}`: owning interviewer only; verifies receipt signature and content hash before returning private media.
- `GET /api/v1/sessions/{session}/integrity`: each observation now includes an `attachments` array with asset IDs, MIME types, byte counts, timestamps, hashes and signatures. Existing risk/annotation fields remain.

Limits and model/calibration behavior are in the linked guide. Evidence attachments do not count as candidate answers.

### Job question-set pool

`POST /api/v1/jobs` accepts `preparation_duration_minutes` (5–180; default 30) and `question_set_count` (1–5; default 1), and atomically queues that many reusable sets. `GET /api/v1/jobs/{job}/question-sets` returns owner-only status and questions/reference rubrics; `POST` retries failed sets. Invitations require a completed matching set and link it without copying questions or generating replacements. See [details](job-question-sets.md).

### Incremental generation progress

Generation v4 saves fully validated questions individually. Owner generation/status and pool previews may show saved questions while status is running/failed. Partial rows do not imply completion. Candidate session responses include `questions_ready`; starting requires a completed assigned set. `POST /jobs/{job}/question-sets` retries failed sets after their saved positions. Pool status includes owner-only `last_failure` summaries/findings, without the raw diagnostic draft.

### Invitation readiness

Creating a candidate invitation requires a completed, nonempty question set for the same job and duration, within the configured set count. Otherwise the API returns HTTP 409 and rolls back candidate/session/token creation. A successful invitation assigns that reusable set immediately. Replaying an existing request key still recovers the original invitation. The interviewer UI disables invitations until a ready set is available. Existing historical waiting invitations are preserved.
