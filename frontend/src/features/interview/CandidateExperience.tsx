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
    try { setSession(await api.transition(action)); }
    catch (error) { setError(error instanceof Error ? error.message : 'Unable to update your interview.'); }
    finally { setBusy(false); }
  }
  if (session?.state === 'in_progress') return <InterviewRoom token={token} initial={session} onSession={setSession} />;
  const finished = !!session && ['submitted', 'scoring', 'completed'].includes(session.state);
  const drafts = session ? pendingDrafts(session.id) : [];
  return <main className="access-page"><section className="access-card candidate-card"><p className="eyebrow">{finished ? 'INTERVIEW SUBMITTED' : 'INTERVIEWEE ACCESS'}</p><h1>{session?.job_title ?? 'Your next opportunity.'}</h1>
    {error && <p className="error" role="alert">{error}</p>}
    {busy && !session && <p role="status">Opening your invitation…</p>}
    {session && <>
      {finished ? <><p className="success" role="status">Your saved answers have been submitted.</p><p>{session.answer_count} of {session.question_count} questions answered.</p><p className="muted">Your interviewer can now review the responses. You can close this page.</p><UnsubmittedRecordings session={session.id} />{drafts.length > 0 && <details className="unsubmitted-drafts"><summary>Local drafts that may not have reached the server</summary><p>These drafts are retained on this device for your records. They are not part of the submitted answers.</p>{drafts.map(draft => <pre key={draft.question}>{draft.content}</pre>)}</details>}</> : session.state === 'expired' ? <p>This invitation has expired. Please contact your interviewer.</p> : <>
        <p>{session.duration_minutes} minutes · {session.question_count} prepared questions</p>
        <ul className="interview-instructions"><li>Answer one question at a time. You may return to earlier questions.</li><li>Your answers save automatically. Wait for “All changes saved” before closing the tab.</li><li>The timer starts when you click Start interview and continues through refreshes and disconnections.</li><li>At the deadline, the server submits your saved answers.</li></ul>
        {session.state === 'invited' ? <button className="button" disabled={busy} onClick={() => void transition('ready')}>{busy ? 'Accepting…' : 'Accept invitation'}</button> : (session.questions_ready ?? session.question_count > 0) ? <button className="button" disabled={busy} onClick={() => void transition('start')}>{busy ? 'Starting…' : 'Start interview →'}</button> : <div className="configuration-note" role="status"><strong>Invitation accepted.</strong><p>{session.generation_status === 'failed' ? 'Question preparation needs attention from your interviewer. Please contact them.' : session.generation_status === 'pending' || session.generation_status === 'running' ? 'Your questions are being prepared. This page updates automatically.' : session.preparation_available === false ? 'Question preparation is not configured yet. Please contact your interviewer.' : 'Preparing your interview questions automatically. This page will update when they are ready.'}</p></div>}
      </>}
    </>}
    {error && <button className="text-button" onClick={() => setAttempt(value => value + 1)}>Retry connection</button>}
  </section></main>;
}
