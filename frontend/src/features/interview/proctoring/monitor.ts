import { SpeechWindow } from './audio';
import { SignalPolicy, type FaceSample, type MediaObservation, type Measurements } from './signals';
import { type evidenceQueue, snapshot, shortClip } from './evidence';
const announce = (kind: string) => window.dispatchEvent(new CustomEvent('interview-device', { detail: kind }));
/** Owns workers/audio processing; never owns or stops the preview's camera/mic tracks. */
export function startMediaMonitor(video: HTMLVideoElement, stream: MediaStream, status: (value: string) => void, queue: ReturnType<typeof evidenceQueue>, live: (value: string) => void = () => {}) {
  let stopped = false; let frameBusy = false; let audioBusy = false; let visionReady = false; let audioReady = false;
  let activeClip: Promise<Blob | null> | null = null;
  let clipCaptured = 0;
  const lastCapture = new Map<string, number>(); let stopClip: (() => void) | undefined;
  let context: AudioContext | null = null; let audioNode: AudioWorkletNode | null = null;
  const policy = new SignalPolicy();
  let speechWindow: SpeechWindow | null = null;
  // Classic workers support MediaPipe's WASM loader; bundled locally by setup:proctoring.
  const base = new URL(`${import.meta.env.BASE_URL}proctoring`, location.origin).href.replace(/\/$/, '');
  let vision: Worker; let audio: Worker;
  try { vision = new Worker(`${base}/inference-worker.js`); }
  catch { announce('model_unavailable'); status('Media checks unavailable · browser monitoring continues'); return { recalibrate() {}, stop() {} }; }
  try { audio = new Worker(`${base}/inference-worker.js`); }
  catch { vision.terminate(); announce('model_unavailable'); status('Media checks unavailable · browser monitoring continues'); return { recalibrate() {}, stop() {} }; }
  const unavailable = new Set<string>();
  function report(message: string) {
    status(unavailable.size ? `${message} · Unavailable checks: ${[...unavailable].join(', ')}` : message);
  }
  function failure(kind: string) { if (stopped || unavailable.has(kind)) return; unavailable.add(kind); announce('model_unavailable'); report(`${kind === 'vision' ? 'Camera analysis' : 'Speech analysis'} unavailable · browser monitoring continues`); }
  async function flag(kind: string, measurements: Measurements) {
    const observation: MediaObservation = { id: crypto.randomUUID(), kind, client_time: Date.now(), measurements };
    const speech = kind === 'possible_background_speech' || kind === 'speech_detected'
      ? speechWindow?.capture(performance.now()) : null;
    // Screen evidence listens for this same event ID, so all assets link to one observation.
    window.dispatchEvent(new CustomEvent('interview-media-observation', { detail: observation }));
    try { await queue.enqueue(observation, []); }
    catch { announce('evidence_capture_failed'); report('Media observation could not be queued'); }
    if (stopped || performance.now() - (lastCapture.get(kind) ?? -Infinity) < 30000) return;
    lastCapture.set(kind, performance.now());
    // Persist the snapshot immediately. A concurrent voice/face flag must not discard it.
    try {
      const captured = Date.now();
      const image = await snapshot(video);
      if (image) await queue.enqueue(observation, [{ id: crypto.randomUUID(), blob: image, captured }]);
    } catch { announce('evidence_capture_failed'); report('Camera snapshot failed · observation retained'); }
    if (stopped) return;
    if (speech) {
      try { await queue.enqueue(observation, [{ id: crypto.randomUUID(), ...speech, phase: 'signal_window' }]); }
      catch { announce('evidence_capture_failed'); report('Speech clip pending or unavailable'); }
      return;
    }
    try {
      // Simultaneous face/voice flags share one recording, each linked to its own event.
      if (!activeClip) {
        clipCaptured = Date.now();
        activeClip = shortClip(new MediaStream(stream.getTracks().filter(track => track.readyState === 'live')), stop => { stopClip = stop; });
        const clearClip = () => { activeClip = null; stopClip = undefined; };
        void activeClip.then(clearClip, clearClip);
      }
      const captured = clipCaptured;
      const clip = await activeClip;
      if (clip) await queue.enqueue(observation, [{ id: crypto.randomUUID(), blob: clip, captured }]);
      else { announce('evidence_capture_failed'); report('Evidence clip unavailable · observation retained'); }
    } catch { announce('evidence_capture_failed'); report('Evidence capture failed · observation retained'); }

  }

  const onFailure = (kind: string) => () => failure(kind);
  vision.onerror = onFailure('vision'); audio.onerror = onFailure('audio');
  vision.onmessage = ({ data }) => {
    if (stopped) return;
    frameBusy = false;
    if (data.type === 'ready') { visionReady = true; report('Look toward the camera for 8 seconds to calibrate eye/head direction'); }
    if (data.type === 'failure') { visionReady = false; failure('vision'); }
    if (data.type === 'face') {
      if (performance.now() - data.at > 2500) return;
      const sample = data.sample as FaceSample;
      live(sample.faces === 0 ? 'No face detected' : sample.faces > 1 ? `${sample.faces} faces detected` : policy.calibrated ? 'One face detected · eye/head checks active' : 'One face detected · calibrating for 8 seconds');
      const wasCalibrated = policy.calibrated;
      for (const event of policy.face(sample, data.at)) void flag(event.kind, event.measurements);
      if (!wasCalibrated && policy.calibrated) report('Face and eye/head direction checks active');
    }
  };
  audio.onmessage = ({ data }) => {
    if (stopped) return;
    audioBusy = false;
    if (data.type === 'ready') { audioReady = true; void setupAudio(); }
    if (data.type === 'failure') { audioReady = false; failure('audio'); }
    if (data.type === 'speech' && performance.now() - data.at <= 2500) { const event = policy.audio(data.probability, data.at); if (event) void flag(event.kind, event.measurements); }
  };
  async function setupAudio() {
    try {
      if (!stream.getAudioTracks().some(track => track.readyState === 'live')) return;
      const created = new AudioContext({ sampleRate: 16000 }); context = created;
      speechWindow = new SpeechWindow(created.sampleRate);
      await created.audioWorklet.addModule(`${base}/pcm-worklet.js`);
      if (stopped) { await created.close(); return; }
      const source = created.createMediaStreamSource(new MediaStream(stream.getAudioTracks()));
      const node = new AudioWorkletNode(created, 'proctor-pcm'); audioNode = node;
      node.port.onmessage = ({ data }) => {
        if (stopped) return;
        speechWindow?.push(data, performance.now());
        if (!stopped && audioReady && !audioBusy) { audioBusy = true; audio.postMessage({ type: 'audio', at: performance.now(), samples: data, sampleRate: created.sampleRate }, [data.buffer]); }
      };
      source.connect(node); node.connect(created.destination); await created.resume();
      report('Speech checks active · speaker identity is not determined');
    } catch { failure('audio'); }
  }
  if (stream.getVideoTracks().some(track => track.readyState === 'live')) vision.postMessage({ type: 'init', kind: 'vision', base });
  if (stream.getAudioTracks().some(track => track.readyState === 'live')) audio.postMessage({ type: 'init', kind: 'audio', base });
  const timeout = setTimeout(() => {
    if (stream.getVideoTracks().length && !visionReady) failure('vision');
    if (stream.getAudioTracks().length && !audioReady) failure('audio');
  }, 30000);
  const timer = setInterval(() => {
    if (stopped || !visionReady || frameBusy || document.hidden || video.readyState < 2 || video.paused) return;
    frameBusy = true;
    void createImageBitmap(video, { resizeWidth: 640, resizeHeight: Math.max(1, Math.round(640 * video.videoHeight / video.videoWidth)) }).then(frame => {
      if (stopped) { frame.close(); return; }
      vision.postMessage({ type: 'frame', at: performance.now(), frame }, [frame]);
    }).catch(() => { frameBusy = false; failure('vision'); });
  }, 200);
  announce('monitoring_started');
  return {
    recalibrate() { policy.recalibrate(); report('Look toward the camera for 8 seconds to recalibrate'); },
    stop() { stopped = true; clearTimeout(timeout); clearInterval(timer); stopClip?.(); speechWindow?.clear(); vision.terminate(); audio.terminate(); audioNode?.disconnect(); void context?.close(); announce('monitoring_stopped'); },
  };
}
