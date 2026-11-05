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
