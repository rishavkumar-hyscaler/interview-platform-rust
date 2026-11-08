# Camera/audio proctoring

## Candidate and interviewer flow

The invitation page requests camera/microphone access and records permission outcomes. Local checks start automatically for enabled devices during the interview. The camera panel shows live face count and calibration status. Device denial/off events are recorded; browser monitoring continues. Fullscreen and explicit screen sharing are required for the room, with screenshots of the selected surface linked to browser and media flags.

For eye/head calibration, look toward the camera for approximately eight seconds of valid camera samples; brief blinks are tolerated and gaps over 2.5 seconds restart calibration. Recalibration is available in the panel. Low light, glasses, camera angle, assistive equipment and frame rate affect estimates. This is coarse geometry, not point-of-regard tracking or an assessment of attention, emotions, intent or ability.

The interviewer opens the session's Integrity review → **Camera and audio observations**. Each item includes a timestamp, numeric measurements, receipt-chain status and private snapshot/clip controls. Existing annotation controls dismiss or qualify observations. SSE refreshes the integrity view as events and attachments arrive. JSON exports include attachment IDs, hashes and signed metadata; binary media is fetched separately with interviewer authentication.

## Local processing and thresholds

MediaPipe packages are pinned to `0.10.32`. Face Landmarker and YAMNet files are downloaded from Google's versioned official model URLs and checked against SHA-256 hashes in `frontend/scripts/proctoring-models.json`. Camera frames and PCM samples are processed in classic Web Workers, not sent to an AI/cloud endpoint. `AudioWorklet` collects one-second mono PCM windows. Workers retain no recognition templates or face identity data; raw landmarks are not uploaded. Expression blendshapes are disabled.

| Observation | Gate | Default risk points |
| --- | --- | --- |
| `face_missing` | Zero detected faces for ≥2 seconds | 1 |
| `multiple_faces` | More than one detected face for ≥2 seconds; model returns at most three | 4 |
| `gaze_away` | Calibrated iris/head offset sustained ≥4 seconds | 0 |
| `possible_background_speech` | Speech probability ≥0.65, plus a fresh camera result with no face, multiple faces, or low visible mouth motion, sustained ≥2 seconds | 1 |
| `speech_detected` | Speech ≥0.65 for ≥2 seconds when fresh camera evidence is unavailable | 0 |

These are heuristic observations. Speech classification cannot determine whether a voice belongs to the candidate, another person, TV/audio playback or a synthetic source. Mouth-motion sampling can miss movement and does not establish speaker identity. This implements **possible** background-speech analysis, not diarization or a reliable count of speakers. The reviewer assistant is explicitly instructed against attention/emotion/intent/identity inference from these signals.

Each signal has a 60-second cooldown. Sampling gaps over 2.5 seconds reset sustained-condition gates. Hidden/paused video is not treated as a fresh missing-face result. The face model samples up to five times per second, with one inference in flight; inference runs off the UI thread. Gaze requires both open/visible eyes and calibration; horizontal iris change >0.18 eye-width units, vertical change >0.09 eye-width units or horizontal head offset >0.16 face-width units / vertical head offset >0.12 face-height units can trigger the gate.

Risk points are configurable with `INTEGRITY_WEIGHTS`, remain separate from rubric scores, and are capped at 100 as review priorities. A flag, model probability or signed receipt is not proof of cheating.

## Evidence capture, storage and recovery

After a sustained media flag, save an aspect-preserving camera JPEG (up to 640 pixels wide) and a shared-screen screenshot if sharing is active after fullscreen entry. Face/look-away flags get a three-second post-signal audio/video clip. Concurrent face flags share one recording, each linked to its own observation. Speech/background-speech flags save up to three preceding seconds as mono PCM16 WAV from the same microphone windows used for classification. Only three one-second windows are held in memory, discarded on stop; no continuous audio is uploaded. If the speech buffer is unavailable, a post-signal clip is attempted. Each kind has a separate 30-second capture cooldown; different flags do not suppress each other's snapshots or speech clips.

Evidence uses the existing `audit_events` table plus private files under `MEDIA_STORAGE_DIR`; no new tables or migrations were added. Camera/audio files have an `.evidence` suffix; screen JPEGs use a session `screenshots/` directory and are separate from answer recordings. They never count as answers and never enter automated answer grading.

The backend requires an authenticated candidate and an existing recent `proctor.observed` event in that candidate's session, of an allowed media-signal kind. Limits: 512 KiB JPEG, 2 MiB per attachment, one image plus one clip per event per source (screen/camera), 40 camera/audio attachments and 120 screen screenshots per session, and a 15-minute attachment window after server receipt of the event. PCM WAV format/length is validated server-side (mono 16-bit, at most three seconds). Compressed clip duration is limited by the browser; the server checks size/signature/file markers but does not decode codecs or verify duration or scene authenticity. A late attachment can finish after submission if it links to a previously accepted event within that window; new observations still require an active interview.

Attachments include the event ID, parent receipt signature, content hash and their own HMAC receipt. Private reads verify both the signed metadata and file SHA-256. Reads require the owning interviewer; candidates cannot browse proctor evidence using interviewer endpoints. Retries use the same asset/event IDs; changed content conflicts. Receipt-chain verification orders batched events by server sequence, not random UUIDs.

IndexedDB retains unsent observations and attachments for up to 14 minutes, with at most 20 pending items per session. Previously captured items retry on reconnection/refresh independently of starting new media monitoring. Event acceptance is persisted before attachment upload so retries after submission can complete against an existing receipt. Candidates see pending/capture failures. Browser storage denial, quota exhaustion, crashes during capture or deliberately modified clients can cause missing evidence; an observation without an attachment is shown explicitly as unverified. Expired pending items are removed on a later queue scan.

Server evidence has the same lifecycle as the existing private media store: no automatic deletion job is implemented. Operators must apply their retention policy to private files and audit exports/backups. Changing the stable invitation/signing key invalidates old receipt verification; preserve it alongside protected backups. Do not expose the media directory as static files. Server signatures prove received bytes/metadata have not changed, not that client telemetry was complete or truthful.

## Setup and verification

From `frontend/`, run `npm ci` followed by `npm run setup:proctoring`. `npm run dev` and `npm run build` also run setup automatically. First setup needs network access for the two official model downloads (~8 MB); subsequent setup validates cached files and rebuilds the local worker. WASM files are copied from installed packages. Serve generated `public/proctoring` assets on the same origin; Vite copies them into production `dist/proctoring`. They are generated/ignored in Git. A model-load failure leaves browser monitoring operational with a visible unavailable status.

Validation includes deterministic signal tests; backend authorization, idempotency, attachment limits, hash tampering and event-chain tests; a browser model fixture (`frontend/tests/proctor-browser.html`); and synthetic room/capture/playback fixture (`frontend/tests/proctor-ui.html`). Test fixtures are development-only HTML entries and are not part of the production application build.

Browser smoke tests used a blank frame/silence, Google's official portrait/speech test samples, a tiled portrait for two faces, and generated camera/audio tracks for automatic capture. They did not access the user's real camera/microphone. Representative consented real-world tests are still needed to tune false positives across lighting, devices, glasses, disabilities and speaking styles. Positive sample fixtures are temporary test downloads, not production assets.

Implementation references: [Face Landmarker](https://developers.google.com/edge/mediapipe/solutions/vision/face_landmarker?tab=Web), [Audio Classifier](https://developers.google.com/edge/mediapipe/solutions/audio/audio_classifier?tab=Web).

## Review assistant resilience

Integrity questions use exact session-wide database counts and bounded keyword retrieval without an embedding call. Other review questions retain semantic retrieval and fall back to session-scoped keyword retrieval if embedding fails. Counts include neutral capability/permission events and repeated observations, so they are not confirmed breaches or unique incidents. The configured LLM is still needed for chat replies, and every cited quote is validated against retrieved sources.

## Analysis readiness and event delivery

Camera permission and `monitoring_started` do not establish that inference is functioning. `face_analysis_ready` is recorded only after the first fresh face result; `gaze_calibrated` after baseline calibration; `audio_analysis_ready` after local microphone analysis starts. These status events and model failures are visible alongside media observations and carry no default risk points. Prolonged look-away still requires calibration and eight sustained seconds; missing face requires five sustained seconds, including when the lens is covered (the detector cannot establish why a face is absent).

All signal durations are whole milliseconds. Browser `performance.now()` is fractional; sending those fractions previously failed integer API validation and prevented flags/attachments from reaching the report. Unaccepted IndexedDB records now normalize only their durations before retry, retaining their IDs and capture timestamps. Upload failures show the HTTP status on the candidate page. Historical flags never accepted by the server cannot be reconstructed after submission.

## Copying question text

A copy selection wholly inside the displayed question is recorded as `copy_from_question`. Pasting that same text while that question is current is `paste_from_question`; only whitespace differences are ignored. Opening the question's context menu is `question_context_menu`. These are permitted activities with zero risk points, even when custom weights are configured, and do not trigger anomaly screenshots. Clipboard text stays in browser memory; only event type and character count are recorded. Unobserved, modified, external or other-question pastes remain normal paste observations.

### Analysis health and delivery feedback

The candidate view shows live face count, sustained-condition countdowns, speech
confidence, and the most recent flag. A countdown resets when the condition clears
or samples become stale; short movements are not flagged. Hidden tabs and paused
previews show an explicit paused state. A worker that stops producing fresh results
for five seconds is marked unavailable, rather than silently appearing healthy.
Analysis failures trigger at most three automatic restarts per mounted interview
media view. The candidate can also use **Restart face/voice checks**; restarting
requires fresh gaze calibration.

Evidence uploads retry each queued record independently. A rejected record does not
block subsequent observations, and successful attachments are checkpointed before
retrying remaining attachments. Analysis-ready/calibration events are status-only:
they intentionally have no attached screenshots or clips. These changes do not add
speaker identification; possible background speech remains a face/mouth-and-speech
heuristic, not verified detection of another speaker.

## On-demand live screen (WebRTC)

During an active interview, session details offer **View live screen** / **Stop live view**.
The candidate sees a notice while connecting/viewing. The sender reuses the screen
already selected through the browser screen picker. No camera/audio tracks are sent.
Screen evidence capture continues independently. Closing the viewer closes the peer;
stopping sharing or ending the interview closes the sender. One viewer per session.

The Rust API handles authenticated SDP signaling over short HTTP polls (2 seconds),
not media transport. Every poll checks session ownership and the active deadline;
candidate access derives the session from their invitation token. SDP has a 64 KiB
limit. Ephemeral rooms expire after 20 seconds without viewer heartbeats; no database
migration or live video storage is involved. Server restart requires reconnecting.
This in-memory signaling implementation requires one backend process (use a shared
signaling store before scaling to multiple instances).

`WEBRTC_ICE_SERVERS` is a JSON array of browser RTCIceServer objects, default `[]`.
Local reachable peers can connect without a relay. Across NAT/firewalls configure
STUN and **TURN**; STUN alone does not guarantee a connection. Example structure:
`[{"urls":"stun:your-host:3478"},{"urls":"turn:your-host:3478","username":"temporary-user","credential":"temporary-password"}]`.
ICE credentials are sent to authorized participating browsers: use limited-lived
TURN credentials, never unrestricted permanent secrets. Production requires HTTPS.
No public third-party ICE service is contacted by default.
