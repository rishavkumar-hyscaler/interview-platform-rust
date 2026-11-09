import { Camera, Mic, Monitor } from 'lucide-react';
import type { InterviewDevices } from './useInterviewDevices';
import type { ScreenEvidence } from './useScreenEvidence';

export function DeviceCheck({ devices, screen }: { devices: InterviewDevices; screen: ScreenEvidence }) {
  const checks = [
    { label: 'Camera', icon: Camera, status: devices.camera, ready: devices.camera === 'enabled', busy: devices.busy, action: 'Allow camera', request: () => devices.request('video') },
    { label: 'Microphone', icon: Mic, status: devices.microphone, ready: devices.microphone === 'enabled', busy: devices.busy, action: 'Allow microphone', request: () => devices.request('audio') },
    { label: 'Screen sharing', icon: Monitor, status: screen.shared ? 'Sharing active' : 'Required to start', ready: screen.shared, busy: screen.busy, action: screen.busy ? 'Opening picker…' : screen.shared ? 'Change screen' : 'Share screen', request: screen.request },
  ];

  return <section className="candidate-check" aria-labelledby="candidate-check-title">
    <h2 id="candidate-check-title">Device and screen check</h2>
    <p>Share your screen to start. Camera and microphone access support local face and speech checks. Permission denials are logged. Your interviewer can view the shared screen live during the interview.</p>
    <ul>
      {checks.map(({ label, icon: Icon, status, ready, busy, action, request }) => <li key={label}>
        <Icon size={20} aria-hidden="true" />
        <div><strong>{label}</strong><small data-ready={ready}>{status.replaceAll('_', ' ')}</small></div>
        {(!ready || label === 'Screen sharing') && <button type="button" className="button secondary" disabled={busy} onClick={() => void request()}>{action}</button>}
      </li>)}
    </ul>
    {devices.error && <p role="status" className="configuration-note">{devices.error}</p>}
    {screen.error && <p className="error" role="alert">{screen.error}</p>}
    <details>
      <summary>What your interviewer can see</summary>
      <p>Choose your entire screen and keep sharing until submission. The app captures screenshots when you enter fullscreen and on flagged browser activity. Your interviewer can view the shared screen live during the interview, and you’ll see a notice when that happens. Live screen video is not recorded. Flagged camera and audio evidence can also be reviewed.</p>
    </details>
  </section>;
}
