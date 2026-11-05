import { evidenceQueue } from './proctoring/evidence';
import { createPortal } from 'react-dom';
import { useEffect, useRef, useState } from 'react';

import { startMediaMonitor } from './proctoring/monitor';
export function InterviewMedia({ controlsTarget, token, sessionId }: { controlsTarget: HTMLDivElement | null; token: string; sessionId: string }) {
  const video = useRef<HTMLVideoElement>(null);
  const stream = useRef<MediaStream | null>(null);
  const audio = useRef<AudioContext | null>(null);
  const alive = useRef(true);
  const evidence = useRef<ReturnType<typeof evidenceQueue> | null>(null);
  const [evidenceStatus, setEvidenceStatus] = useState('');
  const monitor = useRef<ReturnType<typeof startMediaMonitor> | null>(null);
  const [consent, setConsent] = useState(false);
  const [monitorStatus, setMonitorStatus] = useState('Media monitoring off');
  const [camera, setCamera] = useState(false);
  const [mic, setMic] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [level, setLevel] = useState(0);
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; stream.current?.getTracks().forEach(track => track.stop()); void audio.current?.close(); };
  }, []);
  useEffect(() => {
    if (!mic || !stream.current) return;
    const context = new AudioContext(); audio.current = context;
    const analyser = context.createAnalyser(); analyser.fftSize = 256;
    context.createMediaStreamSource(new MediaStream(stream.current.getAudioTracks())).connect(analyser);
    const data = new Uint8Array(analyser.fftSize);
    const timer = setInterval(() => { analyser.getByteTimeDomainData(data); setLevel(Math.min(100, Math.sqrt(data.reduce((sum, value) => sum + ((value - 128) / 128) ** 2, 0) / data.length) * 400)); }, 100);
    return () => { clearInterval(timer); void context.close(); audio.current = null; };
  }, [mic]);
  useEffect(() => {
    const queue = evidenceQueue(token, sessionId, setEvidenceStatus); evidence.current = queue;
    return () => { queue.stop(); evidence.current = null; };
  }, [token, sessionId]);
  useEffect(() => {
    if (!consent || (!camera && !mic) || !stream.current || !video.current || !evidence.current) return;
    const active = startMediaMonitor(video.current, stream.current, setMonitorStatus, evidence.current);
    monitor.current = active;
    return () => { active.stop(); monitor.current = null; };
  }, [consent, camera, mic, token, sessionId]);
  async function toggle(kind: 'video' | 'audio') {
    const enabled = kind === 'video' ? camera : mic;
    if (enabled) {
      stream.current?.getTracks().filter(track => track.kind === kind).forEach(track => { track.stop(); stream.current?.removeTrack(track); });
      window.dispatchEvent(new CustomEvent('interview-device', { detail: kind === 'video' ? 'camera_disabled' : 'microphone_disabled' }));
      if (kind === 'video') setCamera(false); else { setMic(false); setLevel(0); }
      return;
    }
    setBusy(true); setError('');
    try {
      if (!navigator.mediaDevices?.getUserMedia) throw new Error('Device access requires localhost or HTTPS.');
      const acquired = await navigator.mediaDevices.getUserMedia({ video: kind === 'video', audio: kind === 'audio' });
      if (!alive.current) { acquired.getTracks().forEach(track => track.stop()); return; }
      if (!stream.current) stream.current = new MediaStream();
      acquired.getTracks().forEach(track => {
        stream.current!.addTrack(track);
        track.onended = () => { if (alive.current) { if (kind === 'video') setCamera(false); else setMic(false); window.dispatchEvent(new CustomEvent('interview-device', { detail: kind === 'video' ? 'camera_disabled' : 'microphone_disabled' })); } };
      });
      window.dispatchEvent(new CustomEvent('interview-device', { detail: kind === 'video' ? 'camera_enabled' : 'microphone_enabled' }));
      if (kind === 'video') { if (video.current) video.current.srcObject = new MediaStream(acquired.getVideoTracks()); setCamera(true); }
      else setMic(true);
    } catch (error) { if (error instanceof DOMException && error.name === 'NotAllowedError') window.dispatchEvent(new CustomEvent('interview-device', { detail: kind === 'video' ? 'camera_denied' : 'microphone_denied' })); if (alive.current) setError(error instanceof Error ? error.message : 'Unable to access device.'); }
    finally { if (alive.current) setBusy(false); }
  }
  return <section className="room-media"><div className="camera-preview"><video ref={video} autoPlay muted playsInline hidden={!camera} />{!camera && <div><span aria-hidden="true">◎</span><p>Camera off</p></div>}<small>{consent ? 'Media monitoring enabled' : 'Local preview'}</small></div>{controlsTarget && createPortal(<div className="media-controls"><button className="button secondary" aria-pressed={camera} disabled={busy} onClick={() => void toggle('video')}>{camera ? 'Turn camera off' : 'Enable camera'}</button><button className="button secondary" aria-pressed={mic} disabled={busy} onClick={() => void toggle('audio')}>{mic ? 'Turn mic off' : 'Enable microphone'}</button>{mic && <meter min="0" max="100" value={level} aria-label="Microphone input level" />}</div>, controlsTarget)}<label className="media-consent"><input type="checkbox" checked={consent} onChange={event => setConsent(event.target.checked)} />Enable local face, eye/head direction and speech checks; save flagged snapshots and 3-second clips for interviewer review.</label><small className="muted">Enable camera/microphone above. Models run on this device. Only flagged media is uploaded; no continuous recording, identity or emotion analysis. Direction/speech estimates can be wrong.</small>{consent && <><small role="status">{camera || mic ? monitorStatus : 'Enable a device to start media checks. Browser monitoring continues.'}</small>{camera && <button className="text-button" onClick={() => monitor.current?.recalibrate()}>Recalibrate eye/head direction</button>}</>}{evidenceStatus && <small role="status">{evidenceStatus}</small>}{error && <p className="error" role="alert">{error}</p>}</section>;
}
