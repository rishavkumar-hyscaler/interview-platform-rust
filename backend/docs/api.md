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
| GET, POST | `/jobs` | List jobs / create `{title,description,request_key:UUID}` |
| POST | `/jobs/{id}/invitations` | Create `{candidate_name,candidate_email,duration_minutes,request_key:UUID}` |
| GET | `/sessions` | List owned interviews |
| GET | `/sessions/{id}` | Details and invitation availability |
| POST | `/sessions/{id}/invitation` | Recover the saved link token, or explicitly replace an unaccepted legacy link |
| GET, POST | `/sessions/{id}/generation` | Inspect question preparation / enqueue or retry |
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

A preparation request moves the session's generation status through pending/running/completed/failed. A five-minute lease and fencing token support recovery. Failures retry up to three attempts; explicit retry resets the attempt budget. Schema, evidence quotations, rubric totals, and embedding output are validated before publishing. Exact fingerprints and pgvector cosine similarity reject repeats across sessions for the same job. Each question stores its rubric snapshot and generation provenance.

Provider request formats follow the official [Structured Outputs](https://developers.openai.com/api/docs/guides/structured-outputs?api-mode=responses) and [embeddings](https://developers.openai.com/api/reference/resources/embeddings/methods/create) documentation. No raw provider response or API secret is written to audit records.

### Browser observation capture
`POST /api/v1/candidate/events` accepts 1–40 `{id: UUID, kind, client_time: epoch_ms, count?: number}` observations using the invitation bearer capability. Only active interviews can write. Server timestamps are authoritative receipt times; client times and all browser observations are unverified. UUID retries are deduplicated in the immutable audit log. The session row serializes writes and caps storage at 10,000 observations. Paste events contain character counts, never clipboard text. Existing interviewer audit pagination displays the observation evidence.

The browser queues unsent observations on this device and retries after reconnect. This is best-effort capture, not tamper-proof telemetry: client scripts/storage can be modified and cross-tab delivery is not guaranteed. Fullscreen is requested through an explicit browser gesture; denial keeps monitoring active. Device permission denial is recorded when the device button is used. Face/voice analysis, signed event chains, single-tab enforcement and integrity scoring remain pending.
