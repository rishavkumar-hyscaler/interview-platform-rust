# Reusable job question sets

Creating a job atomically queues the interviewer-selected 1–5 question sets (default 1). The interviewer chooses a preparation duration of 5–180 minutes. Each set uses the job blueprint and sequential question/reference/rubric generation with validation, embedding deduplication and independent review. New questions are scoped to 2–3 minutes, and their count is planned to fit the duration.

The Question sets panel shows status, saved progress, reference answers, time plans and bounded failure details. Generation runs only for job sets. There is no per-session generation fallback or automatic replacement on assignment.

Invitations require a completed, nonempty set matching the duration. The transaction links `interview_sessions.question_set_id` to a reusable set, balancing usage across configured sets. Every question belongs to `interview_questions.set_id`; questions are never copied per candidate. Answers and progress remain scoped by session and question. Prepared sets remain usable with the AI provider offline.

Generation state, initial attempt plus three retries, next-run time and worker leases live only in `question_sets`. Retries preserve saved question checkpoints. After four failed worker attempts, an interviewer can explicitly retry the job's failed sets.

## Storage and API

The single development baseline is `202611040001_initial_schema.sql`. Question text, worked references, rubrics and embeddings are stored in `interview_questions`; candidate answers are in `interview_answers`. The SECURITY INVOKER `session_question_details()` function projects questions through session assignment and preserves row-level security.

- `POST /api/v1/jobs`: accepts `question_set_count` and `preparation_duration_minutes` alongside the job fields.
- `GET /api/v1/jobs/{id}/question-sets`: interviewer-only set status, questions and references.
- `POST /api/v1/jobs/{id}/question-sets`: explicit retry of failed configured sets.
- `GET /api/v1/sessions/{id}/generation`: inspect the assigned set and that candidate's answers; this endpoint does not schedule generation.

No candidate API returns reference answers or rubrics.
