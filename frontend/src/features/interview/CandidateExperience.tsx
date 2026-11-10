import { Button } from '../../components/ui/button';
import { CheckCircle2, Clock, ListChecks } from 'lucide-react';
import './candidate.css';
import { DeviceCheck } from './DeviceCheck';
import { useObservations } from './proctoring/observations';
import { useInterviewDevices } from './useInterviewDevices';
import { useScreenEvidence } from './useScreenEvidence';
import { enterInterviewFullscreen } from './fullscreen';
import { UnsubmittedRecordings } from './UnsubmittedRecordings';
import { useEffect, useMemo, useState } from 'react';
import { ApiError } from '../../api';
import { candidateApi, type CandidateSession } from './api';
import { SingleTab } from './SingleTab';
import { InterviewRoom } from './InterviewRoom';
import { pendingDrafts } from './drafts';

export function CandidateExperience({ token }: { token: string }) {
  return <SingleTab key={token} token={token}><CandidateSessionView token={token} /></SingleTab>;
}
function CandidateSessionView({ token }: { token: string }) {
  const api = useMemo(() => candidateApi(token), [token]);
  const [session, setSession] = useState<CandidateSession | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(true);
  const [attempt, setAttempt] = useState(0);
  const active = !!session && ['invited','ready','in_progress'].includes(session.state);
  const observations = useObservations(token,session?.id,active);
  const devices = useInterviewDevices(session?.id,active,observations.record);
  const cameraTrack = devices.stream.current?.getVideoTracks().find(track => track.readyState === 'live') ?? null;
  const screen = useScreenEvidence(token,session?.id,session?.state==='in_progress',observations,cameraTrack);
  useEffect(()=>{if(session && !active)screen.stop();},[active,session?.id,screen.stop]);
  useEffect(() => {
    if (session?.state === 'in_progress' || ['submitted', 'scoring', 'completed', 'expired'].includes(session?.state ?? '')) return;
    const controller = new AbortController(); let active = true; let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await api.session(controller.signal);
        if (!active) return;
        setSession(next); setError('');
        if (['invited', 'ready'].includes(next.state)) timer = setTimeout(() => void poll(), 5000);
      } catch (error) {
        if (active) setError(error instanceof ApiError && error.status === 401 ? 'This invitation is invalid or has expired. Ask your interviewer for a current link.' : error instanceof Error ? error.message : 'Unable to open your invitation.');
      } finally { if (active) setBusy(false); }
    }
    void poll();
    return () => { active = false; controller.abort(); clearTimeout(timer); };
  }, [api, attempt, session?.state]);
  async function transition(action: 'ready' | 'start') {
    setBusy(true); setError('');
    try { if (action === 'start') { if (!screen.shared) throw new Error('Share your screen before starting the interview.'); await enterInterviewFullscreen(); } setSession(await api.transition(action)); }
    catch (error) { setError(error instanceof Error ? error.message : 'Unable to update your interview.'); }
    finally { setBusy(false); }
  }
  if (session?.state === 'in_progress') return <InterviewRoom token={token} initial={session} onSession={setSession} devices={devices} screen={screen} observations={observations} />;
  const finished = !!session && ['submitted', 'scoring', 'completed'].includes(session.state);
  const drafts = session ? pendingDrafts(session.id) : [];
  return <main className="access-page candidate-access"><section className="access-card candidate-card"><p className="eyebrow">{finished ? 'INTERVIEW SUBMITTED' : session?.state === 'expired' ? 'INVITATION EXPIRED' : 'CANDIDATE INTERVIEW'}</p><h1>{session?.job_title ?? 'Your interview invitation'}</h1>
    {error && <p className="error" role="alert">{error}</p>}
    {busy && !session && <p role="status">Opening your invitation…</p>}
    {session && <>
      {finished ? <><p className="success" role="status"><CheckCircle2 size={18} aria-hidden="true" /> Your saved answers have been submitted.</p><p>{session.answer_count} of {session.question_count} questions answered.</p><p className="muted">Your interviewer can now review the responses. You can close this page.</p><UnsubmittedRecordings session={session.id} />{drafts.length > 0 && <details className="unsubmitted-drafts"><summary>Local drafts that may not have reached the server</summary><p>These drafts are retained on this device for your records. They are not part of the submitted answers.</p>{drafts.map(draft => <pre key={draft.question}>{draft.content}</pre>)}</details>}</> : session.state === 'expired' ? <p>This invitation has expired. Please contact your interviewer.</p> : <>
        <div className="candidate-facts">
          <span><Clock size={18} aria-hidden="true" /> {session.duration_minutes} minutes</span>
          <span><ListChecks size={18} aria-hidden="true" /> {session.question_count} questions</span>
        </div>
        <DeviceCheck devices={devices} screen={screen} />
        <section className="candidate-instructions">
          <h2>Before you start</h2>
          <ul>
            <li>Stay in fullscreen during the interview. You can revisit earlier questions.</li>
            <li>Answers save automatically. Check for “All changes saved” before closing the tab.</li>
            <li>The timer keeps running through refreshes and disconnections. Saved answers are submitted when time runs out.</li>
          </ul>
        </section>
        <div className="candidate-start">
        {session.state === 'invited' ? <Button variant="standard" disabled={busy} onClick={() => void transition('ready')}>{busy ? 'Accepting…' : 'Accept invitation'}</Button> : (session.questions_ready ?? session.question_count > 0) ? <Button variant="standard" disabled={busy || devices.busy || !screen.shared} onClick={() => void transition('start')}>{busy ? 'Starting…' : 'Start interview in fullscreen'}</Button> : <div className="configuration-note" role="status"><strong>Invitation accepted.</strong><p>{session.generation_status === 'failed' ? 'Question preparation needs attention from your interviewer. Please contact them.' : session.generation_status === 'pending' || session.generation_status === 'running' ? 'Your questions are being prepared. This page updates automatically.' : 'Waiting for a prepared question set. Please contact your interviewer if none is available.'}</p></div>}
        </div>
      </>}
    </>}
    {error && <Button variant="text" disabled={busy} onClick={() => { setBusy(true); setAttempt(value => value + 1); }}>Retry connection</Button>}
  </section></main>;
}
