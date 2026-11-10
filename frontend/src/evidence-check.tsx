// Disposable browser fixture. No actual camera, microphone, screen, API or AI access.
import { createRoot } from 'react-dom/client';
import { CandidateExperience } from './features/interview/CandidateExperience';
import './style.css';
const session = {
  id: `synthetic-evidence-${Date.now()}`,
  job_title: 'Local permission and evidence check',
  state: 'invited',
  duration_minutes: 30,
  question_count: 1,
  answer_count: 0,
  generation_status: 'completed',
  questions_ready: true,
  deadline_at: null as string | null,
  server_time: new Date().toISOString(),
  submitted_at: null,
};
const question = {
  id: 'synthetic-question',
  position: 1,
  prompt:
    'Explain **atomicity** briefly.\n\n- Give one example.\n- Mention rollback.',
  kind: 'text',
  difficulty: 'easy',
  answer: '',
  revision: 0,
  saved_at: null,
};
const events: string[] = [];
let screenshots = 0;
const update = () => {
  document.getElementById('checks')!.textContent =
    `Synthetic checks: ${events.filter((e) => e === 'camera_denied').length} camera denial, ${events.filter((e) => e === 'microphone_denied').length} mic denial, ${screenshots} screenshot uploads; ${events.slice(-3).join(', ')}`;
};
const original = window.fetch.bind(window);
window.fetch = async (input, options) => {
  const path = String(input);
  if (!path.includes('/api/')) return original(input, options);
  const reply = (data: unknown) =>
    new Response(JSON.stringify(data), {
      headers: { 'Content-Type': 'application/json' },
    });
  if (path.includes('/evidence/')) {
    screenshots++;
    update();
    return reply({ accepted: true });
  }
  if (path.endsWith('/events')) {
    events.push(
      ...JSON.parse(String(options?.body)).map((e: { kind: string }) => e.kind),
    );
    update();
    return reply({ accepted: true });
  }
  if (path.endsWith('/ready')) {
    session.state = 'ready';
    return reply(session);
  }
  if (path.endsWith('/start')) {
    session.state = 'in_progress';
    session.server_time = new Date().toISOString();
    session.deadline_at = new Date(Date.now() + 1800000).toISOString();
    return reply(session);
  }
  if (path.endsWith('/submit')) {
    session.state = 'submitted';
    return reply(session);
  }
  if (path.includes('/room')) return reply({ session, question });
  if (path.includes('/recordings')) return reply({ recordings: [] });
  if (path.includes('/answers/')) {
    question.answer = JSON.parse(String(options?.body)).content;
    question.revision++;
    session.answer_count = 1;
    return reply({ revision: question.revision, answer_count: 1 });
  }
  return reply(session);
};
const canvas = document.createElement('canvas');
canvas.width = 640;
canvas.height = 360;
const context = canvas.getContext('2d')!;
setInterval(() => {
  context.fillStyle = '#1f6470';
  context.fillRect(0, 0, 640, 360);
  context.fillStyle = 'white';
  context.font = '24px sans-serif';
  context.fillText('Simulated shared screen', 50, 150);
}, 100);
Object.defineProperty(navigator, 'mediaDevices', {
  configurable: true,
  value: {
    getUserMedia: async () => {
      throw new DOMException('Synthetic permission denial', 'NotAllowedError');
    },
    getDisplayMedia: async () => {
      const stream = canvas.captureStream(5);
      const track = stream.getVideoTracks()[0];
      Object.defineProperty(track, 'getSettings', {
        value: () => ({ displaySurface: 'monitor', width: 640, height: 360 }),
      });
      return stream;
    },
  },
});
createRoot(document.getElementById('root')!).render(
  <CandidateExperience token="synthetic-evidence-permissions-only" />,
);
