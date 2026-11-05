# Camera/audio proctoring

## Candidate and interviewer flow

The room's camera panel has a separate **Enable local face, eye/head direction and speech checks** checkbox. Enabling camera/microphone alone remains a local preview/device check. With the checkbox selected and devices enabled, local models start. The candidate can turn off a device or uncheck the option; browser activity monitoring continues. Permission/model failures are recorded as capability events, never cheating findings.

For eye/head calibration, look toward the camera for eight valid samples (approximately eight seconds). Recalibration is available in the panel. Low light, glasses, camera angle, assistive equipment and frame rate affect estimates. This is coarse geometry, not point-of-regard tracking or an assessment of attention, emotions, intent or ability.

The interviewer opens the session's Integrity review → **Camera and audio observations**. Each item includes a timestamp, numeric measurements, receipt-chain status and private snapshot/clip controls. Existing annotation controls dismiss or qualify observations. SSE refreshes the integrity view as events and attachments arrive. JSON exports include attachment IDs, hashes and signed metadata; binary media is fetched separately with interviewer authentication.

## Local processing and thresholds

MediaPipe packages are pinned to `0.10.32`. Face Landmarker and YAMNet files are downloaded from Google's versioned official model URLs and checked against SHA-256 hashes in `frontend/scripts/proctoring-models.json`. Camera frames and PCM samples are processed in classic Web Workers, not sent to an AI/cloud endpoint. `AudioWorklet` collects one-second mono PCM windows. Workers retain no recognition templates or face identity data; raw landmarks are not uploaded. Expression blendshapes are disabled.

| Observation | Gate | Default risk points |
| --- | --- | --- |
| `face_missing` | Zero detected faces for ≥5 seconds | 1 |
| `multiple_faces` | More than one detected face for ≥3 seconds; model returns at most three | 4 |
| `gaze_away` | Calibrated iris/head offset sustained ≥8 seconds | 0 |
| `possible_background_speech` | Speech probability ≥0.65, plus a fresh camera result with no face or low visible mouth motion, sustained ≥3 seconds | 1 |
| `speech_detected` | Speech ≥0.65 for ≥3 seconds when fresh camera evidence is unavailable | 0 |

These are heuristic observations. Speech classification cannot determine whether a voice belongs to the candidate, another person, TV/audio playback or a synthetic source. Mouth-motion sampling can miss movement and does not establish speaker identity. This implements **possible** background-speech analysis, not diarization or a reliable count of speakers. The reviewer assistant is explicitly instructed against attention/emotion/intent/identity inference from these signals.

Each signal has a 60-second cooldown. Sampling gaps over 2.5 seconds reset sustained-condition gates. Hidden/paused video is not treated as a fresh missing-face result. The face model samples approximately once per second; inference runs off the UI thread. Gaze requires both open/visible eyes and calibration; horizontal iris change >0.18 eye-width units, vertical change >0.09 eye-width units or relative nose/head offset change >0.16 face-width units can trigger the gate.

Risk points are configurable with `INTEGRITY_WEIGHTS`, remain separate from rubric scores, and are capped at 100 as review priorities. A flag, model probability or signed receipt is not proof of cheating.

## Evidence capture, storage and recovery

After a sustained media flag, capture a current 640×360 JPEG snapshot and a three-second **post-signal** audio/video clip from the already-enabled devices. There is no continuous/pre-event recording buffer. Speech-only observations without camera context do not trigger media capture. A global 30-second capture cooldown limits concurrent/repeated recordings. Other observations remain recorded even when a capture is throttled.

Evidence uses the existing `audit_events` table plus private files under `MEDIA_STORAGE_DIR`; no new tables or migrations were added. Proctor files have an `.evidence` suffix and are separate from answer recordings. They never count as answers and never enter automated answer grading.

The backend requires an authenticated candidate and an existing recent `proctor.observed` event in that candidate's session, of an allowed media-signal kind. Limits: 512 KiB JPEG, 2 MiB per attachment, one image plus one clip per event, 40 attachments per session, and a 15-minute attachment window after server receipt of the event. Clip duration is limited by the browser; the server checks size/signature/file markers but does not decode codecs or verify duration or scene authenticity. A late attachment can finish after submission if it links to a previously accepted event within that window; new observations still require an active interview.

Attachments include the event ID, parent receipt signature, content hash and their own HMAC receipt. Private reads verify both the signed metadata and file SHA-256. Reads require the owning interviewer; candidates cannot browse proctor evidence using interviewer endpoints. Retries use the same asset/event IDs; changed content conflicts. Receipt-chain verification orders batched events by server sequence, not random UUIDs.

IndexedDB retains unsent observations and attachments for up to 14 minutes, with at most 20 pending items per session. Previously captured items retry on reconnection/refresh independently of starting new media monitoring. Event acceptance is persisted before attachment upload so retries after submission can complete against an existing receipt. Candidates see pending/capture failures. Browser storage denial, quota exhaustion, crashes during capture or deliberately modified clients can cause missing evidence; an observation without an attachment is shown explicitly as unverified. Expired pending items are removed on a later queue scan.

Server evidence has the same lifecycle as the existing private media store: no automatic deletion job is implemented. Operators must apply their retention policy to private files and audit exports/backups. Changing the stable invitation/signing key invalidates old receipt verification; preserve it alongside protected backups. Do not expose the media directory as static files. Server signatures prove received bytes/metadata have not changed, not that client telemetry was complete or truthful.

## Setup and verification

From `frontend/`, run `npm ci` followed by `npm run setup:proctoring`. `npm run dev` and `npm run build` also run setup automatically. First setup needs network access for the two official model downloads (~8 MB); subsequent setup validates cached files and rebuilds the local worker. WASM files are copied from installed packages. Serve generated `public/proctoring` assets on the same origin; Vite copies them into production `dist/proctoring`. They are generated/ignored in Git. A model-load failure leaves browser monitoring operational with a visible unavailable status.

Validation includes deterministic signal tests; backend authorization, idempotency, attachment limits, hash tampering and event-chain tests; a browser model fixture (`frontend/tests/proctor-browser.html`); and synthetic room/capture/playback fixture (`frontend/tests/proctor-ui.html`). Test fixtures are development-only HTML entries and are not part of the production application build.

Browser smoke tests used a blank frame/silence, Google's official portrait/speech test samples, a tiled portrait for two faces, and generated camera/audio tracks for automatic capture. They did not access the user's real camera/microphone. Representative consented real-world tests are still needed to tune false positives across lighting, devices, glasses, disabilities and speaking styles. Positive sample fixtures are temporary test downloads, not production assets.

Implementation references: [Face Landmarker](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker?tab=Web), [Audio Classifier](https://developers.google.com/edge/mediapipe/solutions/audio/audio_classifier?tab=Web).
