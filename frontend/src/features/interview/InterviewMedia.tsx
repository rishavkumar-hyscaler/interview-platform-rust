import { createPortal } from 'react-dom';
import { useEffect, useRef, useState } from 'react';

// Device checks are local only. No media is recorded or uploaded.
export function InterviewMedia({ controlsTarget }: { controlsTarget: HTMLDivElement | null }) {
  const video = useRef<HTMLVideoElement>(null);
  const stream = useRef<MediaStream | null>(null);
  const audio = useRef<AudioContext | null>(null);
  const alive = useRef(true);
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
  async function toggle(kind: 'video' | 'audio') {
    const enabled = kind === 'video' ? camera : mic;
    if (enabled) {
      stream.current?.getTracks().filter(track => track.kind === kind).forEach(track => { track.stop(); stream.current?.removeTrack(track); });
      if (kind === 'video') setCamera(false); else { setMic(false); setLevel(0); }
      return;
    }
    setBusy(true); setError('');
    try {
      if (!navigator.mediaDevices?.getUserMedia) throw new Error('Device access requires localhost or HTTPS.');
      const acquired = await navigator.mediaDevices.getUserMedia({ video: kind === 'video', audio: kind === 'audio' });
      if (!alive.current) { acquired.getTracks().forEach(track => track.stop()); return; }
      if (!stream.current) stream.current = new MediaStream();
      acquired.getTracks().forEach(track => stream.current!.addTrack(track));
      window.dispatchEvent(new CustomEvent('interview-device', { detail: kind === 'video' ? 'camera_enabled' : 'microphone_enabled' }));
      if (kind === 'video') { if (video.current) video.current.srcObject = new MediaStream(acquired.getVideoTracks()); setCamera(true); }
      else setMic(true);
    } catch (error) { if (error instanceof DOMException && error.name === 'NotAllowedError') window.dispatchEvent(new CustomEvent('interview-device', { detail: kind === 'video' ? 'camera_denied' : 'microphone_denied' })); if (alive.current) setError(error instanceof Error ? error.message : 'Unable to access device.'); }
    finally { if (alive.current) setBusy(false); }
  }
  return <section className="room-media"><div className="camera-preview"><video ref={video} autoPlay muted playsInline hidden={!camera} />{!camera && <div><span aria-hidden="true">◎</span><p>Camera off</p></div>}<small>Local preview</small></div>{controlsTarget && createPortal(<div className="media-controls"><button className="button secondary" aria-pressed={camera} disabled={busy} onClick={() => void toggle('video')}>{camera ? 'Turn camera off' : 'Enable camera'}</button><button className="button secondary" aria-pressed={mic} disabled={busy} onClick={() => void toggle('audio')}>{mic ? 'Turn mic off' : 'Test microphone'}</button>{mic && <meter min="0" max="100" value={level} aria-label="Microphone input level" />}</div>, controlsTarget)}<small className="muted">Local device preview. Use Optional audio/video answer to record and upload.</small>{error && <p className="error" role="alert">{error}</p>}</section>;
}
