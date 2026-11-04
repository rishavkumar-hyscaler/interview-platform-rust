# Phase 1 API contract

Staff endpoints use opaque auth-session tokens in `Authorization: Bearer <token>`. Candidate capability tokens work only on candidate endpoints. All resources are scoped to the authorized organization. Errors use `{ "error": "Human-readable message", "code": "conflict" }`; extractor errors are normalized without echoing request bodies.

## Access

The product has recruiter/reviewer accounts and candidate invitation access. A recruiter account can create jobs, invite candidates, and inspect interviews and evidence. There is no owner/admin role or separate read-only reviewer role. Registration accepts only `{email, password}`. Registration, login, and `/api/me` return `role: "recruiter"` and an automatically selected internal `organization_id`; the client never asks users to enter it.

Existing account passwords and interview records are preserved by a compatibility migration. Existing owner/interviewer/reviewer memberships become recruiter memberships. Organization IDs and membership tables remain internal record-isolation details, with no organization-management workflow. Candidate capabilities cannot authenticate to recruiter endpoints.

## Staff routes

All below are prefixed `/api/v1/organizations/{organization_id}`.

| Method | Path | Input / output |
|---|---|---|
| GET | `/summary` | Total jobs, sessions, active sessions, reviewable sessions |
| GET | `/jobs` | Cursor page of job descriptions and invitation counts |
| POST | `/jobs` | `{title, description, request_key: UUID}` → job with pinned JD version |
| POST | `/jobs/{job_id}/invitations` | `{candidate_name, candidate_email, duration_minutes, request_key: UUID}` → `{session, invitation_token}` |
| GET | `/sessions` | Cursor page of candidate sessions |
| POST | `/sessions/{session_id}/invitation` | Recover saved link token for manual sharing; explicit replacement for unaccepted legacy invitations |
| GET | `/sessions/{session_id}` | Session, pinned JD, question/answer counts; no computed scores yet |
| GET | `/audit` | Cursor page; optional `entity_id=UUID` filter |

Lists accept `limit` (1–100, default 20) and `cursor` (UUID returned as `next_cursor`). Responses are `{items: [...], next_cursor: UUID | null}` ordered by creation time and UUID descending. Counts in `/summary` are recruiter totals, independent of pagination. Cursors must come from the same collection and filters; unavailable cursors produce an empty page. Timestamp ties use UUID order, not insertion order.

Creating a job or invitation returns 201, or 200 on an identical retry. Reusing a key with different input returns 409. Job and invitation keys are scoped per organization and resource type. The first JD is version 1; editing/version creation APIs are reserved for a later phase. An invitation pins its JD version permanently.

An invitation capability is random, expires after seven days, and is validated using its SHA-256 hash. A second copy is stored as ChaCha20-Poly1305 authenticated ciphertext in `candidate_access.sealed_token`, with organization/session IDs bound as associated data. `INVITATION_ENCRYPTION_KEY` is a stable private 32-byte hex key held outside the database. Back it up privately; changing or losing it makes saved links unrecoverable. Never commit it or silently replace it.

The first creation and identical retries return the same `invitation_token`. The UI creates `/#invite=<token>` using the current candidate-facing frontend origin and retains the fragment so refreshing or reopening the URL restores candidate access. The candidate entry auto-validates the capability; hash/history changes switch candidate context even within an already-open recruiter tab. Session details expose only `invitation_recoverable` and `invitation_active`, never ciphertext or a token. `POST /sessions/{id}/invitation` verifies recruiter access and active session state, returns `{invitation_token, expires_at, replaced_legacy_link}`, and audits the retrieval without recording the secret. Successful API responses disable caching. Expired/submitted sessions cannot retrieve links, and retrieval never extends expiry.

Pre-upgrade invitations only have hashes. Their original secrets cannot be recovered. The UI explicitly offers a replacement for still-unaccepted (`invited`) legacy sessions. This atomically replaces the old hash and stores encrypted recovery material, invalidating the old link without changing expiry. Accepted/in-progress legacy sessions cannot replace links, preserving candidate access. New saved links remain reusable after invitation acceptance. No email is sent automatically; recruiters can copy the recovered link for manual resending.

## Candidate routes

- `GET /api/v1/candidate/session`: session ID, role title, state, duration, deadline, server time, question count. No other candidate data or rubrics.
- `POST /api/v1/candidate/session/ready`: invited → ready.
- `POST /api/v1/candidate/session/start`: ready → in_progress; requires prepared questions; sets a server deadline once.
- `POST /api/v1/candidate/session/submit`: in_progress → submitted.

Repeating an already completed action is idempotent. Invalid or backwards transitions return 409. Expired capabilities return 401. Past invitation/deadline times reject new transitions even if the expiry worker has not run. Expired active states are also reflected by the candidate read response. Scoring/completed states are reserved for later worker implementations; no public endpoint can set them.

## Durable jobs and audit

Invitations enqueue an `expire_session` job transactionally. Starting changes its due time to the interview deadline. The background loop runs inside the HTTP backend process (no separate worker executable) and polls every two seconds, locks jobs with `FOR UPDATE SKIP LOCKED`, enters tenant scope, expires eligible sessions, and writes an audit event atomically. A crash rolls back work and acknowledgement. Failures retry with increasing delay and stop after five attempts (`status=failed`). Alerting/manual retry tooling is not implemented yet.

The capability lookup and operational job tables are global trusted-backend tables, like auth sessions; jobs hold routing IDs, not candidate text. Domain tables use forced RLS and composite tenant foreign keys. Runtime audit records cannot be updated/deleted. Candidate audit actors are identified in metadata; null actor IDs mean candidate/system, not a recruiter.

## Reserved schema

Competency blueprints, question/rubric snapshots, and revisioned answers have tenant-scoped schema for the next phases. Generation, autosave endpoints, answer submission rules, scoring, media, and proctoring are not implemented in Phase 1.
