# Architecture

One Rust executable serves the API and runs background tasks. Internal modules separate authentication, interviews, database access, invitation encryption, and AI generation. React communicates with the backend over HTTP.

## User types and ownership

`users.role` has exactly two values: `interviewer` and `interviewee`. Interviewers sign in with email/password, create jobs and invitations, and review evidence. Reviewing is an interviewer capability, not a separate user role. Interviewees access their assigned session through private invitation links. An invitation creates or reuses an interviewee profile; it does not assign a password. Email is unique within each role.

There are no organizations or memberships. Jobs belong directly to an interviewer. Interview sessions identify both their interviewer and interviewee. Questions belong to reusable job sets; answers belong to a session and question. Forced PostgreSQL row-level security uses transaction-local `app.interviewer_id` derived from authenticated identity. Composite foreign keys prevent cross-interviewer references. The runtime database role cannot bypass RLS.

## Eight application tables

| Table | Responsibility |
|---|---|
| users | Interviewer/interviewee identity |
| auth_sessions | Hashed interviewer login tokens and interviewee invitation capabilities |
| jobs | Job description and competency blueprint |
| question_sets | Reusable job sets and generation retry/lease state |
| interview_sessions | Participants, assigned set, timing and lifecycle |
| interview_questions | Question snapshots, rubrics, and embedding-backed per-job question bank |
| interview_answers | Revisioned answers |
| audit_events | Append-only action/evidence history |

Authentication and invitation tokens share storage but have distinct authorization paths. Interviewer authentication rejects invitation capabilities. Invitation lookup verifies the interviewee role and restores only that capability's session context. Recoverable invitation secrets use authenticated encryption with a separately configured key. API responses disable caching.

## Background processing

No separate queue table or worker build is required. Generation status, attempts, seed, and reclaimable leases live on question sets. Expiry reads persisted deadlines directly. Workers traverse interviewer identities and enter the same RLS context as API requests; this is deliberately simple for the assignment. A large installation would need a more efficient work-discovery mechanism.

Generation and expiry run concurrently. Generation validates model output and checks exact/semantic duplication against existing questions for the job. Questions, rubrics, and generation completion commit atomically. Shutdown can abandon an AI request safely; its lease makes unfinished work reclaimable. Deadline finalization and its audit record commit together. In-progress interviews submit their saved answers when time ends; unstarted invitations expire. Answer saves use optimistic revisions and serialize against submission, preventing late writes and silent multi-tab overwrites.

## Operational boundaries

Passwords use Argon2id. Opaque login tokens expire after 12 hours and are hashed in PostgreSQL. Recruiter-side browser sessions persist per tab and are validated through `/api/me` after refresh. Passwords are never stored in the browser. Audit records and question snapshots have no runtime update/delete grants. Production still needs TLS, rate limits, account recovery, retention controls, monitoring, and security review.

The browser journals unacknowledged answers locally without storing invitation tokens in that journal. It clears matching drafts only after server acknowledgement, restores pending text after refresh, retries on reconnect, and requires explicit conflict resolution when another tab has changed an answer. The authoritative saved answer, cursor trail, and deadline remain on the server.

## Job identity

`jobs.id` remains the immutable internal UUID. `jobs.job_code` is the interviewer-entered Job ID, normalized to uppercase and constrained unique on `(interviewer_id, job_code)`. Sessions and the question bank retain UUID foreign keys. One additive migration backfills existing jobs with deterministic legacy codes and preserves the applied baseline checksum.
