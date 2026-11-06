/** Geometry and persistence gates only. No identity, emotion, attention or intent inference. */
export type Point = { x: number; y: number; z?: number };
export type FaceSample = { faces: number; eyeX?: number; eyeY?: number; headX?: number; headY?: number; mouth?: number };
export type Measurements = { duration_ms: number; face_count?: number; speech_probability?: number; horizontal_offset?: number; vertical_offset?: number; mouth_motion?: number; head_horizontal_offset?: number; head_vertical_offset?: number };
export type MediaObservation = { id: string; kind: string; client_time: number; measurements?: Measurements };
export function faceGeometry(faces: Point[][]): FaceSample {
  const result: FaceSample = { faces: faces.length };
  if (faces.length !== 1 || faces[0].length < 478) return result;
  const p = faces[0];
  function eye(a: number, b: number, iris: number, top: number, bottom: number) {
    const dx = p[b].x - p[a].x; const dy = p[b].y - p[a].y; const width = dx * dx + dy * dy;
    const height = Math.abs(p[bottom].y - p[top].y);
    if (width < 0.00001 || height < 0.004) return null; // Blinks/occluded eyes are not gaze evidence.
    return { x: ((p[iris].x - p[a].x) * dx + (p[iris].y - p[a].y) * dy) / width, y: (p[iris].y - (p[top].y + p[bottom].y) / 2) / Math.sqrt(width) };
  }
  const left = eye(33, 133, 468, 159, 145); const right = eye(362, 263, 473, 386, 374);
  const width = Math.abs(p[454].x - p[234].x);
  if (left && right) {
    result.eyeX = (left.x + right.x) / 2; result.eyeY = (left.y + right.y) / 2;
  }
  if (width > 0.08) {
    result.headX = (p[1].x - (p[234].x + p[454].x) / 2) / width;
    const height = Math.abs(p[152].y - p[10].y);
    if (height > 0.08) result.headY = (p[1].y - p[10].y) / height;
  }
  const mouthWidth = Math.hypot(p[291].x - p[61].x, p[291].y - p[61].y);
  if (mouthWidth > 0.01) result.mouth = Math.hypot(p[14].x - p[13].x, p[14].y - p[13].y) / mouthWidth;
  return result;
}
export class SustainedSignal {
  private since: number | null = null;
  private lastSample = 0;
  private lastFlag = -Infinity;
  progress(now: number) {
    if (this.since === null || now - this.lastSample > 2500) return 0;
    return Math.min(this.minimumMs, Math.max(0, Math.floor(now - this.since)));
  }
  constructor(private readonly minimumMs: number, private readonly cooldownMs = 60000) {}
  update(active: boolean, now: number): number | null {
    if (now - this.lastSample > 2500) this.since = null;
    this.lastSample = now;
    if (!active) { this.since = null; return null; }
    this.since ??= now;
    const duration = Math.min(120000, Math.floor(now - this.since));
    if (duration < this.minimumMs || now - this.lastFlag < this.cooldownMs) return null;
    this.lastFlag = now; return duration;
  }
}
export class SignalPolicy {
  private absent = new SustainedSignal(2000);
  private multiple = new SustainedSignal(2000);
  private gaze = new SustainedSignal(4000);
  private voice = new SustainedSignal(2000);
  private speech = new SustainedSignal(2000);
  private baseline: number[][] = [];
  private calibrationStarted: number | null = null;
  private calibrationLast = -Infinity;
  private calibrationComplete = false;
  private lastFace: { sample: FaceSample; at: number } | null = null;
  private mouths: { value: number; at: number }[] = [];
  private faceStatus = 'Waiting for camera analysis';
  private voiceStatus = 'Waiting for speech analysis';
  get summary() { return `${this.faceStatus} · ${this.voiceStatus}`; }
  get calibrated() { return this.calibrationComplete; }
  recalibrate() {
    this.baseline = []; this.calibrationStarted = null; this.calibrationLast = -Infinity;
    this.calibrationComplete = false; this.gaze = new SustainedSignal(4000);
  }
  private calibrate(sample: FaceSample, valid: boolean, now: number) {
    if (this.calibrated) return;
    if (now - this.calibrationLast > 2500) {
      this.baseline = []; this.calibrationStarted = null;
    }
    // Brief blinks do not restart calibration; prolonged occlusion/missing faces do.
    if (!valid) return;
    this.calibrationLast = now;
    this.calibrationStarted ??= now;
    this.baseline.push([sample.eyeX!, sample.eyeY!, sample.headX!, sample.headY ?? 0]);
    this.calibrationComplete = this.baseline.length >= 8 && now - this.calibrationStarted >= 7000;
  }
  face(sample: FaceSample, now: number): { kind: string; measurements: Measurements }[] {
    this.lastFace = { sample, at: now };
    // Never compare mouth positions across faces or stale camera samples.
    if (sample.faces !== 1 || !this.mouths.length || now - this.mouths[this.mouths.length - 1].at > 1500) this.mouths = [];
    if (sample.faces === 1 && sample.mouth !== undefined) this.mouths.push({ value: sample.mouth, at: now });
    this.mouths = this.mouths.filter(value => now - value.at < 4000);
    const result: { kind: string; measurements: Measurements }[] = [];
    for (const [gate, kind, active] of [[this.absent, 'face_missing', sample.faces === 0], [this.multiple, 'multiple_faces', sample.faces > 1]] as const) {
      const duration = gate.update(active, now);
      if (duration !== null) result.push({ kind, measurements: { duration_ms: duration, face_count: sample.faces } });
    }
    const valid = sample.faces === 1 && sample.eyeX !== undefined && sample.eyeY !== undefined && sample.headX !== undefined;
    this.calibrate(sample, valid, now);
    const mean = (i: number) => this.baseline.reduce((sum, value) => sum + value[i], 0) / this.baseline.length;
    const horizontal = valid && this.calibrated ? sample.eyeX! - mean(0) : 0;
    const vertical = valid && this.calibrated ? sample.eyeY! - mean(1) : 0;
    const headValid = sample.faces === 1 && sample.headX !== undefined;
    const headHorizontal = headValid && this.calibrated ? sample.headX! - mean(2) : 0;
    const headVertical = headValid && this.calibrated && sample.headY !== undefined ? sample.headY - mean(3) : 0;
    const turned = Math.abs(headHorizontal) > 0.16 || Math.abs(headVertical) > 0.12;
    const duration = this.gaze.update(this.calibrated && (turned || (valid && (Math.abs(horizontal) > 0.18 || Math.abs(vertical) > 0.09))), now);
    if (duration !== null) result.push({ kind: 'gaze_away', measurements: { duration_ms: duration, face_count: 1, horizontal_offset: horizontal, vertical_offset: vertical, head_horizontal_offset: headHorizontal, head_vertical_offset: headVertical } });
    const seconds = (gate: SustainedSignal, total: number) => `${(gate.progress(now) / 1000).toFixed(1)}/${total}s`;
    this.faceStatus = sample.faces === 0 ? `No face: ${seconds(this.absent, 2)}`
      : sample.faces > 1 ? `${sample.faces} faces: ${seconds(this.multiple, 2)}`
      : !this.calibrated ? 'One face · calibrating (look toward camera for 8 seconds)'
      : turned || (valid && (Math.abs(horizontal) > 0.18 || Math.abs(vertical) > 0.09))
        ? `Eye/head direction away: ${seconds(this.gaze, 4)}` : 'One face · direction within baseline';
    return result;
  }
  audio(probability: number, now: number): { kind: string; measurements: Measurements } | null {
    const face = this.lastFace && now - this.lastFace.at < 2500 ? this.lastFace.sample : null;
    const mouths = this.mouths.filter(value => now - value.at < 4000);
    const motion = mouths.length >= 3 && mouths[mouths.length - 1].at - mouths[0].at >= 2000 && now - mouths[mouths.length - 1].at < 1500 ? Math.max(...mouths.map(v => v.value)) - Math.min(...mouths.map(v => v.value)) : null;
    const possibleBackground = probability >= 0.65 && !!face && (face.faces !== 1 || (face.faces === 1 && motion !== null && motion < 0.025));
    const duration = this.voice.update(possibleBackground, now);
    const speechDuration = this.speech.update(probability >= 0.65 && !face, now);
    this.voiceStatus = `Speech ${Math.round(probability * 100)}%` + (possibleBackground
      ? ` · possible background speech ${(this.voice.progress(now) / 1000).toFixed(1)}/2s`
      : !face && probability >= 0.65 ? ` · speech ${(this.speech.progress(now) / 1000).toFixed(1)}/2s` : '');
    if (duration !== null) return { kind: 'possible_background_speech', measurements: { duration_ms: duration, speech_probability: probability, face_count: face!.faces, ...(motion === null ? {} : { mouth_motion: motion }) } };
    return speechDuration === null ? null : { kind: 'speech_detected', measurements: { duration_ms: speechDuration, speech_probability: probability } };
  }
}
