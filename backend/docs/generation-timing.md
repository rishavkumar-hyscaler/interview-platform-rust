# Staged question preparation (v3)

The former pipeline generated every question together with worked answers and rubrics, then checked duplicates and reviewed the complete set. A rejected question caused another full, expensive set-generation request. Worker retries could repeat that again.

The new order is:

1. Extract or reuse a validated job blueprint. Cache new valid blueprints before reference generation.
2. Generate **only prompts**, with type, competency, difficulty and estimated time.
3. Check blueprint alignment, lengths, question count, total duration and duplicate fingerprints within the set.
4. Reject exact job-bank duplicates before requesting embeddings. Check semantic similarity within the draft and against every existing bank question (including unused pool sets).
5. Review prompt relevance, feasibility, ambiguity and answer privacy. Invalid prompts can receive one bounded repair. **No reference-answer request starts unless the whole prompt set passes.**
6. Generate references/rubrics for individual approved questions, with three tasks in flight by default. Each task validates and independently reviews its own answer. A rejected reference receives one repair with specific findings; the other approved prompts and successful references are retained in that attempt.
7. Assemble in original blueprint order, validate the completed set, then publish atomically. The final transaction rechecks job-bank deduplication under the existing job lock to catch concurrent publications.

Reference responses cannot contain replacement prompts: their schema only permits answer/rubric fields, and the server attaches them to immutable approved prompt fields. Neither prompts alone nor incomplete references become candidate-visible interview questions.

## Time and retries

- `GENERATION_REFERENCE_CONCURRENCY`: default **3**, bounded 1–6. It caps generation and review together, rather than starting six unconstrained calls.
- `GENERATION_TASK_TIMEOUT_SECONDS`: default **300 seconds per preparation attempt**, bounded 30–900. The timer covers blueprint, prompts, embeddings, reviews, reference work and publication. Cancellation aborts child reference futures.
- Each prompt/reference stage permits at most two local tries. Quality/validation failures after those repairs and task timeouts become `failed`; they do not automatically restart the whole set three more times. Interviewers can explicitly retry from the existing UI.
- Transient provider/network/database failures retain the existing bounded worker retry behavior. Thus the per-attempt budget is not a guarantee of total wall time across an outage, queue wait or manual retries.
- Existing per-request timeout and reasoning-effort settings remain supported. Smaller prompt-only and single-reference response budgets replace the old full-set answer output budget.

Logs expose phases (`questions.generate`, `questions.validate`, `questions.deduplicate`, `questions.review`, `references.generate_and_review`, `reference.repair`, `publish`) and reference-position spans, without logging prompts, answer keys or provider keys.

No migration or additional process/build is required. Existing completed questions remain intact. Work interrupted under the previous version can be retried through the normal failed-set control after the backend restarts.

## Verification and limits

The local mock-provider/database regressions verify zero reference calls for invalid, duplicate or rejected prompts; zero embedding calls for exact bank collisions; three bounded concurrent reference tasks; repair of only the failing reference; prompt immutability; reference privacy; and atomic publication. The tests also retain pool assignment, lease recovery and session behavior checks.

These tests establish request ordering and bounds, not a live-model speedup ratio. The v3 pipeline has not been benchmarked against the live provider during this change; the user's backend remains stopped. External model latency, coding complexity, rate limits and quality repairs still affect completion time. Approved prompt/reference state is held in memory during an attempt; a process crash or transient failure requiring a new attempt can repeat that work. The validated blueprint is durable.

## Reference metadata validation

Single-question reference review returns content verdicts only, without a model-authored question number. Rust attaches the known position and assembles results in approved prompt order. Rubric criterion IDs have the same ASCII/length constraints in the structured-output schema and server validation; errors distinguish malformed IDs, duplicate IDs, invalid descriptions and invalid point values so local repairs receive actionable feedback. No scoring or reference-correctness checks are bypassed.
