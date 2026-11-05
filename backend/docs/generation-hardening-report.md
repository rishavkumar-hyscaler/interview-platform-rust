# Question-generation hardening report

Date: 5 November 2026

## Status
Code changes are implemented and local/mock regression checks pass. Live end-to-end generation is not yet verified successfully. The final isolated acceptance run exhausted its bounded retries. Its latest rejection was `Quality review rejected question 3: unresolved concerns`. No rejected question set was published. Do not treat this as production acceptance.

## Implemented
- JD-grounded competency blueprints, normalized internal identifiers and bounded blueprint repair.
- Exact question/slot/review counts in the structured-output schema, with length/range constraints and required nullable fields.
- Worked reference answers with assumptions, acceptable alternatives and concrete mistakes.
- Criterion-keyed reference sections assembled by the server, with partial/full-credit anchors and a ten-point total.
- Coding reference solutions, complexity notes and typical/edge-case examples.
- Separate AI quality review for relevance, correctness, alignment, feasibility, ambiguity and answer privacy.
- Bounded repair using the rejected draft and validation feedback; actual duplicate-bank collisions supplied as exclusions.
- Exact and semantic deduplication, with final transactional publication checks.
- Renewable worker leases, configurable generation timeouts and safe provider-error classification.
- Question-before-answer schema authoring order, efficient gpt-5-mini drafting and separate medium-effort critique.
- Interviewer reference display and scoring guidance that accepts valid alternative approaches.

## Verification
- 19 unit tests passed.
- PostgreSQL and local mock-provider integration regressions passed, including rejection, repair, deduplication, access isolation and candidate answer-key privacy.
- Rust Clippy, frontend build, frontend lint and diff checks passed.
- Live trials exposed schema, alignment, identifier, count and timeout issues; fixes were added with regression coverage. Later retries reached the quality gate but were rejected for reference correctness, ambiguity or unresolved concerns. Live acceptance remains incomplete.

## Remaining work
1. Diagnose/reproduce the live lease-loss failure and verify recovery without duplicate publication.
2. Complete a successful real-provider six-question generation and manually inspect its reference answers/code examples.
3. Confirm the expanded reference display with a successful live set.

## Compatibility and limits
No new tables or migration files are required. Existing prepared interviews are preserved; the new format applies to newly prepared sets after backend restart. Reference code is AI-reviewed but is not compiled or executed by the platform. Model review is not a proof of factual or functional correctness.
