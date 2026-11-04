# Assignment implementation status

Stack follows the user's explicit correction: Rust, React, PostgreSQL and pgvector. One backend executable, seven application tables and one baseline migration remain.

| Requirement | Implementation |
| --- | --- |
| JD blueprint, seeded unique questions, exact/vector dedup, rubric snapshots | Generation worker and interviewer preparation view |
| Adaptive difficulty | Next question evaluates the saved answer and chooses a suitable unserved question from the prepared set; manual question navigation remains available |
| Timed room, cursor resume, text/code, autosave | Server deadline, optimistic revisions, local unsent draft recovery, refreshed cursor |
| Fullscreen and one browser tab | Editor requires fullscreen; exclusive Web Lock gates the candidate view on one origin/profile |
| Browser proctoring | Focus/visibility/fullscreen, copy/paste count, context menu, typing bursts, rapid answer timing, window-size dev-tools suspicion, supported display-extension signal |
| Immutable evidence | Server timestamp, event IDs, idempotency, measurement units, signed receipt chain, RLS-protected append-only audit |
| AI scoring | Rubric point bounds, exact evidence quotes, blank-answer zeroes, per-question live progress, final competency profile and human-review recommendation |
| Reviewer assistant | Scoped embeddings/pgvector retrieval, evidence paging, exact citations, per-conversation memory, genuine token streaming and final citation validation |
| Integrity review | Configurable risk weights, evidence list, activity vs suspicion distinction, audited reviewer overrides, AI evidence-summary action |
| Live reviewer | SSE replay/cursors, heartbeat, credential checks, reconnect and provisional scoring events |
| Analytics and observability | Job question-bank descriptive groups and exploratory correlation; scoped latency percentiles, counts, request/worker traces |
| Export | Full JSON report with answer/rubric/audit/score/integrity evidence |
| Bias and injection guardrails | Model receives job-relevant prompts/rubrics/answers, not candidate profile; no tools are exposed. Citations and point limits are validated; human judgement remains necessary |

## Explicit limits

- Optional recorded audio/video answers and optional face/look-away/background-voice analysis are not implemented. Camera/mic currently provide local previews and device checks without upload. Browser monitoring continues if device access is denied.
- Web Locks cannot coordinate different origins, profiles, browsers or devices. They prevent accidental duplicate tabs, not malicious API clients.
- Web apps cannot reliably detect every display or dev-tools instance. Unsupported browser signals are unavailable; window-size heuristics can produce false positives. Typing bursts may come from assistive tools, input methods or accessibility software.
- A server signature proves receipt integrity, not the truth or completeness of client telemetry. Browser-side suppression cannot be ruled out.
- RAG operates on explicit bounded evidence pages, not an unbounded model context. Citations prove that quoted text exists, not that a model's interpretation is necessarily correct. Provisional streamed text is clearly labelled and discarded when final validation fails.
- AI scoring and recommendations support a human reviewer. Rubric/identity bias and statistical validity still need evaluation on representative, consented data; small-sample analytics are not validated discrimination or fairness measures.
- Live integration is covered with a local mock provider. Current provider keys/model availability and real-world camera/browser compatibility need deployment acceptance testing.
