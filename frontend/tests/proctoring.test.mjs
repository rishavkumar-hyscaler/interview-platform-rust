import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { Buffer } from 'node:buffer';
import ts from 'typescript';
const source = readFileSync(new URL('../src/features/interview/proctoring/signals.ts', import.meta.url), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText;
const { SustainedSignal, SignalPolicy, faceGeometry } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);
const neutral = { faces: 1, eyeX: 0.5, eyeY: 0, headX: 0, mouth: 0.02 };
test('sustained signals reject brief changes, sampling gaps and cooldown repeats', () => {
  const gate = new SustainedSignal(3000, 60000);
  assert.equal(gate.update(true, 0), null); assert.equal(gate.update(true, 1000), null);
  assert.equal(gate.update(false, 2000), null);
  for (const time of [3000, 4000, 5000]) assert.equal(gate.update(true, time), null);
  assert.equal(gate.update(true, 6000), 3000);
  assert.equal(gate.update(true, 7000), null);
  assert.equal(new SustainedSignal(3000).update(true, 10000), null);
  const gapped = new SustainedSignal(3000); gapped.update(true, 0); assert.equal(gapped.update(true, 5000), null);
});
test('face absence and multiple faces require sustained observations', () => {
  const policy = new SignalPolicy(); let found = [];
  for (let time = 0; time <= 5000; time += 1000) found.push(...policy.face({ faces: 0 }, time));
  assert.deepEqual(found.map(event => event.kind), ['face_missing']);
  found = []; for (let time = 6000; time <= 9000; time += 1000) found.push(...policy.face({ faces: 2 }, time));
  assert.deepEqual(found.map(event => event.kind), ['multiple_faces']);
});
test('gaze needs calibration, sustained offset and resets on invalid eyes', () => {
  const policy = new SignalPolicy();
  for (let i = 0; i < 8; i++) assert.deepEqual(policy.face(neutral, i * 1000), []);
  assert.equal(policy.calibrated, true);
  let found = []; for (let i = 8; i <= 16; i++) found.push(...policy.face({ ...neutral, eyeX: 0.8 }, i * 1000));
  assert.deepEqual(found.map(event => event.kind), ['gaze_away']);
  policy.recalibrate(); assert.equal(policy.calibrated, false);
  assert.deepEqual(faceGeometry([]), { faces: 0 }); assert.deepEqual(faceGeometry([[]]), { faces: 1 });
});
test('audio distinguishes speech from possible off-camera speech without identifying speakers', () => {
  const policy = new SignalPolicy(); let found;
  for (let i = 0; i <= 6; i++) { policy.face(neutral, i * 1000); found = policy.audio(0.8, i * 1000) ?? found; }
  assert.equal(found.kind, 'possible_background_speech');
  const speaking = new SignalPolicy(); let falseFlag = null;
  for (let i = 0; i <= 8; i++) { speaking.face({ ...neutral, mouth: i % 2 ? 0.3 : 0.02 }, i * 1000); falseFlag ??= speaking.audio(0.9, i * 1000); }
  assert.equal(falseFlag, null);
  const noCamera = new SignalPolicy(); let speech;
  for (let i = 0; i <= 4; i++) speech = noCamera.audio(0.9, i * 1000) ?? speech;
  assert.equal(speech.kind, 'speech_detected');
});
test('fast camera sampling does not finish calibration in 1.4 seconds; blinks are tolerated', () => {
  const policy = new SignalPolicy();
  for (let time = 0; time <= 1400; time += 200) policy.face(neutral, time);
  assert.equal(policy.calibrated, false);
  for (let time = 1600; time <= 7200; time += 200) policy.face(time === 4000 ? { faces: 1 } : neutral, time);
  assert.equal(policy.calibrated, true);
  policy.recalibrate();
  for (let time = 10000; time <= 11000; time += 200) policy.face(neutral, time);
  for (let time = 15000; time <= 16000; time += 200) policy.face(neutral, time);
  assert.equal(policy.calibrated, false);
});
test('vertical head movement has its own evidence and brief look-away does not flag', () => {
  const policy = new SignalPolicy();
  for (let i = 0; i < 8; i++) policy.face({ ...neutral, headY: 0.5 }, i * 1000);
  let events = [];
  for (let i = 8; i <= 16; i++) events.push(...policy.face({ ...neutral, headY: 0.7 }, i * 1000));
  assert.equal(events.length, 1);
  assert.equal(events[0].kind, 'gaze_away');
  assert.ok(events[0].measurements.head_vertical_offset > 0.12);
  const brief = new SignalPolicy();
  for (let i = 0; i < 8; i++) brief.face(neutral, i * 1000);
  for (let i = 8; i < 12; i++) assert.deepEqual(brief.face({ ...neutral, eyeX: 0.8 }, i * 1000), []);
  assert.deepEqual(brief.face(neutral, 12000), []);
});
test('multiple faces plus speech is flagged for review; stale mouths do not imply background speech', () => {
  const policy = new SignalPolicy(); let event;
  for (let i = 0; i <= 4; i++) { policy.face({ faces: 2 }, i * 1000); event = policy.audio(0.9, i * 1000) ?? event; }
  assert.equal(event.kind, 'possible_background_speech');
  assert.equal(event.measurements.face_count, 2);
  const stale = new SignalPolicy();
  for (let i = 0; i <= 2; i++) stale.face(neutral, i * 1000);
  for (let i = 3; i <= 7; i++) { stale.face({ faces: 1 }, i * 1000); assert.equal(stale.audio(0.9, i * 1000), null); }
});
test('head turns remain detectable when irises are occluded', () => {
  const policy = new SignalPolicy();
  for (let i = 0; i < 8; i++) policy.face(neutral, i * 1000);
  let events = [];
  for (let i = 8; i <= 16; i++) events.push(...policy.face({ faces: 1, headX: 0.3 }, i * 1000));
  assert.equal(events[0]?.kind, 'gaze_away');
});
test('long-running observations stay within the server duration limit', () => {
  const gate = new SustainedSignal(3000);
  let duration;
  for (let time = 0; time <= 200000; time += 1000) duration = gate.update(true, time) ?? duration;
  assert.equal(duration, 120000);
});
const audioSource = readFileSync(new URL('../src/features/interview/proctoring/audio.ts', import.meta.url), 'utf8');
const audioCompiled = ts.transpileModule(audioSource, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText;
const { SpeechWindow } = await import(`data:text/javascript;base64,${Buffer.from(audioCompiled).toString('base64')}`);
test('speech evidence includes triggering audio in a bounded playable PCM WAV', async () => {
  const window = new SpeechWindow(16000);
  for (let i = 0; i < 5; i++) window.push(new Float32Array(16000).fill(i === 4 ? 0.5 : -0.5), i * 1000);
  const clip = window.capture(4000);
  assert.equal(clip.blob.type, 'audio/wav');
  const bytes = new DataView(await clip.blob.arrayBuffer());
  assert.equal(bytes.byteLength, 44 + 16000 * 2 * 3);
  assert.equal(bytes.getUint32(24, true), 16000);
  assert.equal(bytes.getInt16(44, true), -16384);
  assert.equal(bytes.getInt16(bytes.byteLength - 2, true), 16383);
  assert.equal(window.capture(8000), null);
  window.clear(); assert.equal(window.capture(4000), null);
});
test('overlapping face/voice flags retain their own images and shared clips', async () => {
  const keys = ['window', 'Worker', 'location', 'performance', 'CustomEvent', 'MediaStream'];
  const original = Object.fromEntries(keys.map(key => [key, globalThis[key]]));
  let now = 0; const sent = []; const screen = []; const workers = []; let finishClip;
  globalThis.window = new EventTarget();
  globalThis.CustomEvent = class extends Event { constructor(type, options) { super(type); this.detail = options.detail; } };
  globalThis.location = { origin: 'http://localhost' };
  globalThis.performance = { now: () => now };
  globalThis.Worker = class { constructor() { workers.push(this); } postMessage() {} terminate() {} };
  globalThis.MediaStream = class { constructor(tracks) { this.tracks = tracks; } };
  globalThis.__proctorCaptureTest = {
    snapshot: async () => new Blob(['image'], { type: 'image/jpeg' }),
    shortClip: () => new Promise(resolve => { finishClip = resolve; }),
  };
  const moduleUrl = text => `data:text/javascript;base64,${Buffer.from(text).toString('base64')}`;
  const evidence = moduleUrl('export const snapshot = globalThis.__proctorCaptureTest.snapshot; export const shortClip = globalThis.__proctorCaptureTest.shortClip;');
  const monitorSource = readFileSync(new URL('../src/features/interview/proctoring/monitor.ts', import.meta.url), 'utf8');
  const monitor = ts.transpileModule(monitorSource, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText
    .replaceAll("'./signals'", JSON.stringify(moduleUrl(compiled)))
    .replaceAll("'./audio'", JSON.stringify(moduleUrl(audioCompiled)))
    .replaceAll("'./evidence'", JSON.stringify(evidence))
    .replaceAll('import.meta.env.BASE_URL', '"/"');
  let running;
  try {
    window.addEventListener('interview-media-observation', event => screen.push(event.detail));
    const { startMediaMonitor } = await import(moduleUrl(monitor));
    const track = { readyState: 'live' };
    const stream = { getVideoTracks: () => [track], getAudioTracks: () => [track], getTracks: () => [track] };
    running = startMediaMonitor({ paused: true }, stream, () => {}, { enqueue: async (observation, assets) => { sent.push({ observation, assets }); } });
    for (now = 0; now <= 5000; now += 1000) {
      workers[0].onmessage({ data: { type: 'face', at: now, sample: { faces: 0 } } });
      workers[1].onmessage({ data: { type: 'speech', at: now, probability: 0.9 } });
      await new Promise(resolve => setImmediate(resolve));
    }
    finishClip(new Blob(['clip'], { type: 'video/webm' }));
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(screen.map(event => event.kind).sort(), ['face_missing', 'possible_background_speech']);
    for (const event of screen) {
      const assets = sent.filter(item => item.observation.id === event.id).flatMap(item => item.assets);
      assert.deepEqual(assets.map(asset => asset.blob.type).sort(), ['image/jpeg', 'video/webm']);
    }
  } finally {
    running?.stop(); Object.assign(globalThis, original); delete globalThis.__proctorCaptureTest;
  }
});
test('fractional browser clocks produce integer durations accepted by the API', () => {
  const policy = new SignalPolicy(); let events = [];
  for (let i = 0; i <= 26; i++) events.push(...policy.face({ faces: 0 }, 100.33 + i * 200.07));
  assert.equal(events.length, 1);
  assert.equal(events[0].kind, 'face_missing');
  assert.equal(events[0].measurements.duration_ms, 5001);
  assert.equal(Number.isInteger(events[0].measurements.duration_ms), true);
});
