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
