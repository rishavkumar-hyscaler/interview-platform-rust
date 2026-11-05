import { SignalPolicy, type FaceSample, type MediaObservation, type Measurements } from './signals';
import { type evidenceQueue, snapshot, shortClip } from './evidence';
const announce = (kind: string) => window.dispatchEvent(new CustomEvent('interview-device', { detail: kind }));
/** Owns workers/audio processing; never owns or stops the preview's camera/mic tracks. */
export function startMediaMonitor(video: HTMLVideoElement, stream: MediaStream, status: (value: string) => void, queue: ReturnType<typeof evidenceQueue>) {
  let stopped = false; let frameBusy = false; let audioBusy = false; let visionReady = false; let audioReady = false;
  let capturing = false; let lastCapture = -Infinity; let stopClip: (() => void) | undefined;
  let context: AudioContext | null = null; let audioNode: AudioWorkletNode | null = null;
  const policy = new SignalPolicy();
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
    // Send the event immediately; binary attachments follow after capture and survive retries separately.
    try { await queue.enqueue(observation, []); } catch { announce('evidence_capture_failed'); report('Media observation could not be queued'); }
    if (kind === 'speech_detected' || capturing || performance.now() - lastCapture < 30000 || stopped) return;
    capturing = true; lastCapture = performance.now();
    try {
      const image = await snapshot(video);
      if (stopped) return;
      const clip = await shortClip(new MediaStream(stream.getTracks().filter(track => track.readyState === 'live')), stop => { stopClip = stop; });
      const assets = [image, clip].filter((value): value is Blob => !!value).map(blob => ({ id: crypto.randomUUID(), blob }));
      if (!assets.length) announce('evidence_capture_failed');
      await queue.enqueue(observation, assets);
    } catch { announce('evidence_capture_failed'); report('Evidence capture failed · observation retained'); }
    finally { capturing = false; stopClip = undefined; }
  }
  const onFailure = (kind: string) => () => failure(kind);
  vision.onerror = onFailure('vision'); audio.onerror = onFailure('audio');
  vision.onmessage = ({ data }) => {
    if (stopped) return;
    frameBusy = false;
    if (data.type === 'ready') { visionReady = true; report('Look toward the camera for 8 seconds to calibrate eye/head direction'); }
    if (data.type === 'failure') { visionReady = false; failure('vision'); }
    if (data.type === 'face') {
      const wasCalibrated = policy.calibrated;
      for (const event of policy.face(data.sample as FaceSample, performance.now())) void flag(event.kind, event.measurements);
      if (!wasCalibrated && policy.calibrated) report('Face and eye/head direction checks active');
    }
  };
  audio.onmessage = ({ data }) => {
    if (stopped) return;
    audioBusy = false;
    if (data.type === 'ready') { audioReady = true; void setupAudio(); }
    if (data.type === 'failure') { audioReady = false; failure('audio'); }
    if (data.type === 'speech') { const event = policy.audio(data.probability, performance.now()); if (event) void flag(event.kind, event.measurements); }
  };
  async function setupAudio() {
    try {
      if (!stream.getAudioTracks().some(track => track.readyState === 'live')) return;
      const created = new AudioContext({ sampleRate: 16000 }); context = created;
      await created.audioWorklet.addModule(`${base}/pcm-worklet.js`);
      if (stopped) { await created.close(); return; }
      const source = created.createMediaStreamSource(new MediaStream(stream.getAudioTracks()));
      const node = new AudioWorkletNode(created, 'proctor-pcm'); audioNode = node;
      node.port.onmessage = ({ data }) => {
        if (!stopped && audioReady && !audioBusy) { audioBusy = true; audio.postMessage({ type: 'audio', samples: data, sampleRate: created.sampleRate }, [data.buffer]); }
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
    void createImageBitmap(video, { resizeWidth: 640, resizeHeight: 360 }).then(frame => {
      if (stopped) { frame.close(); return; }
      vision.postMessage({ type: 'frame', frame }, [frame]);
    }).catch(() => { frameBusy = false; failure('vision'); });
  }, 1000);
  announce('monitoring_started');
  return {
    recalibrate() { policy.recalibrate(); report('Look toward the camera for 8 seconds to recalibrate'); },
    stop() { stopped = true; clearTimeout(timeout); clearInterval(timer); stopClip?.(); vision.terminate(); audio.terminate(); audioNode?.disconnect(); void context?.close(); announce('monitoring_stopped'); },
  };
}
