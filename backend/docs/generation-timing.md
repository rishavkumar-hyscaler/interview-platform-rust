## Automatic retry policy

Every generation failure now receives up to three automatic worker retries after the initial attempt (four total attempts), including validation rejection, duplicates, provider errors, unexpected response formats, and unexpected preparation panics. Retry delays are 5, 10 and 15 seconds. Task attempts and next-run times are stored in PostgreSQL, so restarting the backend does not reset the budget. A failed preparation task cannot terminate the worker loop; cancellation also aborts its in-flight work.

Saved checkpoints and the time plan are reused. The next worker attempt receives persisted review findings and the previous rejected draft. Existing bounded local draft/repair calls remain inside each worker attempt, so four worker attempts does not mean only four provider requests. After the fourth failure, the set requires an explicit interviewer retry. Existing failed records are not silently restarted by this code change. No live-provider tests were run.

## Fixed time planning (v6)

Before drafting any question, the backend plans all remaining slots and persists the plan as a `generation.planned` audit event for the set/session. New questions get fixed allocations of 2–3 minutes. A 10-minute fresh interview uses at most five 2-minute questions; a 30-minute interview with six questions uses up to eighteen task minutes, leaving review time. Priority for a third minute combines answer type and difficulty. Code scope is one small function or a focused fix in supplied starter code (normally 3–8 changed lines); scenario/text scope is one decision/concept answered in 2–4 sentences. Full components, services and multi-feature workflows are excluded.

The blueprint request count is capped by duration / 2. Older cached blueprints are adapted locally to fit, keeping higher-priority competencies. Existing saved questions and their times are preserved. A legacy partial set may therefore retain some earlier one-minute questions; only new questions follow the 2–3 minute policy. A set-specific plan is reused across retries, so cached job blueprints and saved questions do not need paid regeneration. The strict output schema fixes estimated_minutes to the planned allocation. Quality review checks scope and correctness independently. A time plan does not guarantee that every person will finish in exactly that time.

The interviewer can expand Question time plan to see the number of questions, each allocation and scope, and the remaining review time. No new database table or migration is required.

## Current flow: fewer requests (v5)

Job creation accepts `question_set_count` (1–5, default 1). This is the total number of job sets, including assigned/pending/failed work; completed sets are reused across candidates without queuing replacements. interview_sessions.question_set_id links to the source questions directly; no question copies are created. session_question_details() is a SECURITY INVOKER read function preserving table RLS on PostgreSQL 14 and later. Candidate answers, revisions and progress are scoped by session_id and question_id. Session-specific generation beyond these sets requires the interviewer’s explicit Prepare action. Existing jobs keep their previous target of three. The target is returned by job and pool APIs. Changing it on an existing job is not exposed yet.

For each unsaved position, generate the complete question, reference answer and rubric in one call. Validate its structure, time allocation and exact fingerprint in Rust before requesting an embedding. Check semantic similarity against the job bank, then use one independent content review for both prompt and reference. Save only the fully validated question before proceeding.

A successful six-question set uses 6 draft calls + 6 embedding calls + 6 review calls, plus one blueprint call when the job has no cached blueprint. Each question allows one duplicate replacement and one content repair at most (three drafts maximum); unchanged prompts reuse their embedding during content repair; it does not regenerate previous checkpoints. Invalid local drafts and duplicates skip review. Transient service errors can still trigger the existing bounded task retries, so total usage can be higher on failures. No live AI test was run for this change.

The combined draft may spend answer tokens on a question that is subsequently rejected as a duplicate. This trades some wasted tokens on duplicate drafts for fewer requests on successful questions.

The sections below describe earlier implementation stages and are retained as history.

# Sequential question preparation (v4)

A set is prepared **one question at a time**. The backend completes its checks and saves its reference before beginning the next question. Each accepted question is a durable checkpoint in `interview_questions`; no additional table or migration is required.

## Flow

1. Extract or reuse the job's validated competency blueprint. The blueprint is saved before question work begins.
2. Load already saved question positions and time allocations for the set/session. Check that positions are contiguous, then start at the first unsaved position.
3. Allocate a share of the remaining interview time to the current blueprint slot.
4. Generate exactly one prompt, without an answer or rubric. Validate its type, competency, difficulty, lengths and time allocation.
5. Check exact fingerprints against the full job bank before requesting embeddings, then check semantic similarity against existing bank questions, including earlier checkpoints in this set.
6. Review the single prompt for relevance, feasibility, clarity and answer privacy. The model returns only content verdicts; Rust supplies question identity. A rejected prompt gets one bounded local repair.
7. Generate that prompt's reference answer and rubric. Validate the schema, criterion IDs/points/anchors, alternatives, mistakes and code references where applicable. Independently review reference correctness; a rejection gets one local repair with specific findings.
8. Save the fully approved prompt, reference, rubric and embedding in one transaction, under the active task lease and per-job publication lock. Recheck bank deduplication at save time. Record `question.prepared` in the audit log.
9. Begin the next unsaved position. Only the last checkpoint marks the whole set/session generation `completed` and records `questions.prepared`.

There is no parallel question/reference generation within a task. A process failure, lease recovery, manual retry or later question rejection preserves earlier checkpoints and their IDs. An unfinished current prompt/reference can be repeated; earlier saved questions are not regenerated. Attempts still describe the overall worker attempt, while saved positions describe progress within it.

## Readiness and access

Interviewers can see saved questions and references while preparation is running or failed. A partial set is not ready for assignment to a candidate. For session-specific preparation, the candidate cannot start the interview until generation is completed; merely having some question rows is insufficient. The candidate API's `questions_ready` flag and the start endpoint enforce this. Pre-existing manually prepared legacy sessions without generation metadata remain supported.

## Time and retries

- `GENERATION_TASK_TIMEOUT_SECONDS`: default **300 seconds per current question**, bounded 30–900. It covers prompt generation, checks, embeddings, reviews, reference work and its checkpoint transaction. Timing out preserves earlier checkpoints and fails the task for explicit retry.
- Each prompt/reference gets at most two local tries. Exhausted validation/quality failures do not automatically restart the whole set repeatedly.
- Transient provider/network/database failures retain bounded worker retries. A recovered attempt loads saved positions before calling the model.
- `GENERATION_REQUEST_TIMEOUT_SECONDS` still controls individual model requests. `GENERATION_DRAFT_REASONING_EFFORT` applies to drafting, with separate medium-effort reference review.
- The previous concurrency setting is no longer used: sequential preparation is intentional. Total set latency is the sum of its question times; the benefit is early visible progress and durable recovery, not a promised speedup ratio.

Logs identify phases and question positions without raw prompts, reference answers or provider keys. Rejected attempts additionally store **private owner-scoped** `generation.rejected` audit records, with the phase, position, summary, structured reviewer findings and a bounded draft for diagnosis. These details are not sent to candidate APIs. The job pool endpoint exposes only the summary/findings subset, and its UI displays concrete concerns. Existing failures predating this change cannot retroactively supply findings that were never saved.

## Validation

Synthetic-data/mock-provider database tests cover question-by-question generation, an intentional failure on question 3 after two checkpoints, recovery following an expired worker lease, unchanged saved IDs, no regenerated references for saved questions, partial pool-set visibility/recovery, rejection of premature candidate start, and private diagnostic access. The normal authorization, scoring, media and interview regressions remain in place.

An initial diagnostic replay was blocked by automatic approval review. After the user explicitly approved one isolated live test, a reduced fixture used only the job description, blueprint and existing prompts, excluding candidate answers and existing reference answers. The disposable database and fixture were removed afterward.

That test stopped after 134 seconds with zero newly saved questions: the model proposed a full React tag-input component for a five-minute slot, and reference review judged its scope closer to 10–15 minutes. The failure also exposed nonblocking implementation suggestions mixed into blocking concerns. The latest changes constrain short code tasks to a narrow function, focused bug fix or scaffolded change; distinguish `advisories` from substantive `concerns`; and route prompt defects found during reference review back to bounded prompt regeneration for the current unsaved position. Reference-correctness defects still repair the reference. A per-question time budget covers both scope attempts.

Synthetic regression tests verify that only the infeasible unsaved prompt is replaced, saved checkpoints remain untouched, advisory notes do not block otherwise valid references, and genuine correctness failures still reject publication. The latest scope/advisory changes have not had a second live-provider run; no full-set live acceptance or speedup claim is made.

## Recovery after an unresolvable reference

The live job later reached question 2 in two sets, preserving question 1 in each, but reference review continued to reject IAM policy details after answer repair. Recovery now also replaces the current **unsaved** prompt when two reference content reviews fail. The replacement must assess the same blueprint slot and differ from the rejected prompt; it must pass prompt review, deduplication and reference review from scratch. Provider/transport errors and rubric-format errors are not treated as content rejections for this fallback. There are at most two question-scope attempts inside the current question's existing time budget. Rejected answers are never saved, and prior checkpoints are untouched.

Prompt instructions prefer applied reasoning or facts supplied by the task over unsupported recall of vendor action catalogs or syntax. This is not automatic external-document retrieval; the final content review remains model-based. The new branch is covered by a synthetic factual-error-on-question-2 regression, not a second live-provider test. Reference rejection snapshots now retain the approved prompt alongside the draft reference so future investigations can compare findings against the actual task contract.
