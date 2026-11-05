# Assignment implementation status

Stack follows the user's explicit correction: Rust, React, PostgreSQL and pgvector. One backend executable, seven application tables, one baseline migration and one additive Job ID migration remain.

| Requirement | Implementation |
| --- | --- |
| JD blueprint, seeded unique questions, exact/vector dedup, rubric snapshots | Generation v2: worked references, assumptions, alternatives, mistakes, criterion-keyed reference sections, partial/full-credit anchors, coding examples, deterministic checks, a separate AI critic, bounded repairs and renewed worker leases |
| Adaptive difficulty | Next question evaluates the saved answer and chooses a suitable unserved question from the prepared set; manual question navigation remains available |
| Timed room, cursor resume, text/code/media, autosave | Server deadline, optimistic revisions, local unsent draft recovery, refreshed cursor; optional bounded audio/video attachments with private playback and IndexedDB retry recovery |
| Fullscreen and one browser tab | Editor requires fullscreen; exclusive Web Lock gates the candidate view on one origin/profile |
| Media proctoring | Opt-in local face counts, calibrated eye/head geometry, possible background-speech analysis, event-linked private snapshots/clips and interviewer playback; see [details and limitations](media-proctoring.md) |
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

- Optional audio/video answers and opt-in local media proctoring are implemented. Eye/head direction and possible background-speech flags are heuristic estimates, not attention/intent inference, speaker identification or cheating verdicts. Only event-triggered proctor snapshots/clips upload automatically after opt-in. Browser monitoring continues if device/model access is unavailable. Media-only answers still require human review, not an automatic zero. See [media proctoring](media-proctoring.md).
- Web Locks cannot coordinate different origins, profiles, browsers or devices. They prevent accidental duplicate tabs, not malicious API clients.
- Web apps cannot reliably detect every display or dev-tools instance. Unsupported browser signals are unavailable; window-size heuristics can produce false positives. Typing bursts may come from assistive tools, input methods or accessibility software.
- A server signature proves receipt integrity, not the truth or completeness of client telemetry. Browser-side suppression cannot be ruled out.
- RAG operates on explicit bounded evidence pages, not an unbounded model context. Citations prove that quoted text exists, not that a model's interpretation is necessarily correct. Provisional streamed text is clearly labelled and discarded when final validation fails.
- AI scoring and recommendations support a human reviewer. Rubric/identity bias and statistical validity still need evaluation on representative, consented data; small-sample analytics are not validated discrimination or fairness measures.
- Recording capture/upload/playback and local recovery have been smoke-tested with synthetic video/audio tracks, without accessing real camera/microphone hardware. Live AI integration is covered with a local mock provider. Current provider keys/model availability and real-world camera/browser compatibility need deployment acceptance testing.

Private media files live under `MEDIA_STORAGE_DIR` (default `backend/storage/media`). Back up this directory alongside PostgreSQL and the stable invitation key. Do not expose it through a static web server. JSON exports include audit references and content hashes, not the binary recordings. Browser recording is capped at 60 seconds, server size at 15 MB and server count at 12 clips per session. There is no server-side codec decoding or duration verification.

Job creation now queues three unique question/reference sets before invitations; consuming a matching set replenishes the pool. See [job-set behavior and validation](job-question-sets.md). This timing is an agreed workflow extension to the per-session uniqueness requirement.
