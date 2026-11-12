/** Isolated test fixture: synthetic camera/audio and mocked API; never touches real devices. */
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { InterviewMedia } from '../src/features/interview/InterviewMedia';
import {
  ProctorEvidence,
  type ProctorAttachment,
} from '../src/features/workspace/ProctorEvidence';
import '../src/style.css';
const canvas = document.createElement('canvas');
canvas.width = 640;
canvas.height = 360;
const context = canvas.getContext('2d')!;
context.fillStyle = '#e8ece5';
setInterval(() => context.fillRect(0, 0, 640, 360), 100);
const OriginalWorker = window.Worker;
window.Worker = class extends OriginalWorker {
  constructor(url: string | URL, options?: WorkerOptions) {
    super(url, options);
    this.addEventListener('message', ({ data }) => {
      if (data.type === 'speech' || data.type === 'face')
        document.documentElement.dataset[`${data.type}Samples`] = String(
          Number(document.documentElement.dataset[`${data.type}Samples`] ?? 0) +
            1,
        );
    });
  }
};
const blobs = new Map<string, Blob>();
const session = crypto.randomUUID();
type Event = {
  id: string;
  received_at: string;
  evidence: { kind: string };
  attachments: ProctorAttachment[];
};
let receive: (kind: string, data: unknown) => void = () => {};
const original = window.fetch.bind(window);
window.fetch = async (input, options) => {
  const path = String(input);
  if (
    path.includes('/api/v1/candidate/events/') &&
    path.includes('/evidence/') &&
    options?.body instanceof Blob
  ) {
    const asset = path.split('/').at(-1)!;
    blobs.set(asset, options.body);
    receive('attachment', {
      event: path.split('/')[5],
      asset_id: asset,
      mime: options.body.type,
      bytes: options.body.size,
      saved_at: new Date().toISOString(),
    });
    return Response.json({ accepted: true });
  }
  if (path === '/api/v1/candidate/events') {
    receive('events', JSON.parse(String(options?.body)));
    return Response.json({ accepted: true });
  }
  if (path.includes('/api/v1/sessions/') && path.includes('/evidence/'))
    return new Response(blobs.get(path.split('/').at(-1)!), {
      headers: { 'Content-Type': blobs.get(path.split('/').at(-1)!)!.type },
    });
  return original(input, options);
};
Object.defineProperty(navigator.mediaDevices, 'getUserMedia', {
  value: async (constraints: MediaStreamConstraints) => {
    if (constraints.video) return canvas.captureStream(10);
    const audio = new AudioContext();
    const oscillator = audio.createOscillator();
    const gain = audio.createGain();
    gain.gain.value = 0;
    const destination = audio.createMediaStreamDestination();
    oscillator.connect(gain);
    gain.connect(destination);
    oscillator.start();
    destination.stream.getTracks()[0].onended = () => {
      oscillator.stop();
      void audio.close();
    };
    return destination.stream;
  },
});
function Harness() {
  const [target, setTarget] = useState<HTMLDivElement | null>(null);
  const [events, setEvents] = useState<Event[]>([]);
  receive = (kind, value) => {
    if (kind === 'events') {
      for (const event of value as { id: string; kind: string }[])
        setEvents((previous) =>
          previous.some((item) => item.id === event.id)
            ? previous
            : [
                ...previous,
                {
                  id: event.id,
                  received_at: new Date().toISOString(),
                  evidence: { kind: event.kind },
                  attachments: [],
                },
              ],
        );
    } else {
      const asset = value as ProctorAttachment & { event: string };
      setEvents((previous) =>
        previous.map((item) =>
          item.id === asset.event
            ? { ...item, attachments: [...item.attachments, asset] }
            : item,
        ),
      );
    }
  };
  return (
    <main style={{ maxWidth: 1000, margin: '1rem auto' }}>
      <h1>Synthetic media test</h1>
      <p>No real camera/microphone is used.</p>
      <div ref={setTarget} />
      <InterviewMedia
        controlsTarget={target}
        token="synthetic-test-token"
        sessionId={session}
      />
      <p id="capture-result">
        {JSON.stringify(
          events.map((event) => ({
            kind: event.evidence.kind,
            assets: event.attachments.map((asset) => asset.mime),
          })),
        )}
      </p>
      <ProctorEvidence
        events={events}
        session={session}
        token="synthetic-test-token"
      />
    </main>
  );
}
createRoot(document.getElementById('root')!).render(<Harness />);
