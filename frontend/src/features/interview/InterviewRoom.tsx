import { useEffect, useMemo, useRef, useState } from 'react';
import { ApiError } from '../../api';
import { candidateApi, type CandidateSession, type Question } from './api';
import { Proctoring } from './Proctoring';
import { InterviewMedia } from './InterviewMedia';
import { clearDraft, readDraft, writeDraft } from './drafts';
const time = (ms: number) => { const seconds = Math.max(0, Math.floor(ms / 1000)); return `${Math.floor(seconds / 60).toString().padStart(2, '0')}:${(seconds % 60).toString().padStart(2, '0')}`; };

export function InterviewRoom({ token, initial, onSession }: { token: string; initial: CandidateSession; onSession: (session: CandidateSession) => void }) {
  const api = useMemo(() => candidateApi(token), [token]);
  const [session, setSession] = useState(initial);
  const [question, setQuestion] = useState<Question | null>(null);
  const [content, setContent] = useState('');
  const [submittedContent, setSubmittedContent] = useState('');
  const [posting, setPosting] = useState(false);
  const [mediaControls, setMediaControls] = useState<HTMLDivElement | null>(null);
  const [saving, setSaving] = useState('Loading saved answers…');
  const [error, setError] = useState('');
  const [conflict, setConflict] = useState(false);
  const [loading, setLoading] = useState(true);
  const [finishing, setFinishing] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [localWarning, setLocalWarning] = useState('');
  const [now, setNow] = useState(Date.parse(initial.server_time));
  const [questionStarted, setQuestionStarted] = useState(0);
  const clock = useRef({ epoch: Date.parse(initial.server_time), at: performance.now() });
  const questionRef = useRef<Question | null>(null);
  const textRef = useRef('');
  const revisionRef = useRef(0);
  const dirtyRef = useRef(false);
  const conflictRef = useRef(false);
  const inFlight = useRef<Promise<void> | null>(null);
  const mounted = useRef(true);
  const loadSerial = useRef(0);
  const finishAttempt = useRef(false);
  const submitDialog = useRef<HTMLDialogElement>(null);
  const remaining = Math.max(0, Date.parse(session.deadline_at ?? session.server_time) - now);
  function retain(value: string) {
    const current = questionRef.current;
    if (current && !writeDraft(session.id, current.id, { content: value, revision: revisionRef.current })) setLocalWarning('Local recovery is unavailable. Keep this tab open until your answer is saved.');
  }
  async function load(position?: number) {
    const serial = ++loadSerial.current;
    setLoading(true); setError('');
    try {
      const result = await api.room(position);
      if (!mounted.current || serial !== loadSerial.current) return;
      if (!result.question) { onSession(result.session); return; }
      clock.current = { epoch: Date.parse(result.session.server_time), at: performance.now() };
      setNow(clock.current.epoch); setSession(result.session);
      questionRef.current = result.question; revisionRef.current = result.question.revision;
      const draft = readDraft(result.session.id, result.question.id);
      const recover = !!draft && draft.content !== result.question.answer;
      const changedElsewhere = recover && draft.revision !== result.question.revision;
      const value = recover ? draft.content : result.question.answer;
      if (draft && !recover) clearDraft(result.session.id, result.question.id, draft.content);
      textRef.current = value; dirtyRef.current = recover; conflictRef.current = changedElsewhere;
      setContent(value); setSubmittedContent(result.question.answer); setQuestion(result.question); setQuestionStarted(Date.parse(result.question_started_at ?? result.session.server_time));
      setConflict(changedElsewhere);
      setSaving(changedElsewhere ? 'Resolve the saved version below.' : recover ? 'Recovered an unsaved draft. Saving…' : 'All changes saved');
      if (changedElsewhere) setError('This answer changed in another tab. Your local draft is preserved.');
    } catch (error) { if (mounted.current && serial === loadSerial.current) setError(error instanceof Error ? error.message : 'Unable to load your interview.'); }
    finally { if (mounted.current && serial === loadSerial.current) setLoading(false); }
  }
  async function flush(): Promise<void> {
    if (inFlight.current) { await inFlight.current; return flush(); }
    if (!dirtyRef.current) return;
    if (conflictRef.current) throw new Error('Resolve the answer conflict before continuing.');
    const current = questionRef.current;
    if (!current) return;
    const snapshot = textRef.current; const revision = revisionRef.current;
    setSaving('Saving…');
    const task = (async () => {
      try {
        const receipt = await api.save(current.id, snapshot, revision);
        revisionRef.current = receipt.revision;
        if (textRef.current === snapshot) {
          dirtyRef.current = false; clearDraft(session.id, current.id, snapshot);
        } else retain(textRef.current);
        if (mounted.current) { setSaving(dirtyRef.current ? 'Unsaved changes' : 'All changes saved'); setError(''); setSession(value => ({ ...value, answer_count: receipt.answer_count })); }
      } catch (error) {
        if (mounted.current) {
          if (error instanceof ApiError && error.status === 409) { conflictRef.current = true; setConflict(true); }
          setSaving('Not saved yet · draft kept on this device'); setError(error instanceof Error ? error.message : 'Unable to save your answer.');
        }
        throw error;
      }
    })();
    inFlight.current = task;
    try { await task; } finally { inFlight.current = null; }
    if (dirtyRef.current) return flush();
  }
  useEffect(() => {
    mounted.current = true; void load();
    return () => { mounted.current = false; loadSerial.current++; };
    // A room instance belongs to one invitation; callbacks read the live refs.
  }, [api]);
  useEffect(() => {
    if (!loading && dirtyRef.current && !conflict) {
      const timer = setTimeout(() => { void flush().catch(() => {}); }, 650);
      return () => clearTimeout(timer);
    }
  }, [content, loading, conflict]);
  useEffect(() => {
    const timer = setInterval(() => setNow(clock.current.epoch + performance.now() - clock.current.at), 1000);
    return () => clearInterval(timer);
  }, []);
  useEffect(() => {
    const beforeUnload = (event: BeforeUnloadEvent) => { if (dirtyRef.current) { event.preventDefault(); event.returnValue = ''; } };
    const online = () => { if (dirtyRef.current && !conflictRef.current) void flush().catch(() => {}); };
    window.addEventListener('beforeunload', beforeUnload); window.addEventListener('online', online);
    return () => { window.removeEventListener('beforeunload', beforeUnload); window.removeEventListener('online', online); };
  });
  useEffect(() => {
    const retry = setInterval(() => {
      const serverNow = clock.current.epoch + performance.now() - clock.current.at;
      if (dirtyRef.current && !conflictRef.current && !inFlight.current && Date.parse(initial.deadline_at ?? '') > serverNow) void flush().catch(() => {});
    }, 5000);
    return () => clearInterval(retry);
  }, [api, initial.deadline_at]);
  async function finish(timed = false) {
    setFinishing(true); setError('');
    try {
      if (timed) { try { await flush(); } catch { /* Server decides whether a late save is accepted. */ } }
      else await flush();
      onSession(await api.transition('submit'));
    } catch (error) { setError(error instanceof Error ? error.message : 'Unable to confirm submission. Reconnect and retry.'); }
    finally { if (mounted.current) setFinishing(false); }
  }
  useEffect(() => {
    if (remaining <= 0 && !finishAttempt.current) { finishAttempt.current = true; void finish(true); }
  }, [remaining]);
  useEffect(() => {
    if (confirm) submitDialog.current?.showModal(); else submitDialog.current?.close();
  }, [confirm]);
  async function navigate(position: number) {
    try { await flush(); await load(position); } catch { /* Keep the current draft and show its save error. */ }
  }
  async function resolve(keepLocal: boolean) {
    const current = questionRef.current; if (!current) return;
    setLoading(true);
    try {
      const fresh = await api.room(current.position);
      if (!fresh.question) { onSession(fresh.session); return; }
      revisionRef.current = fresh.question.revision;
      conflictRef.current = false; setConflict(false); setError('');
      if (keepLocal) { retain(textRef.current); await flush(); }
      else { clearDraft(session.id, current.id, textRef.current); dirtyRef.current = false; textRef.current = fresh.question.answer; setContent(fresh.question.answer); setSaving('All changes saved'); }
    } catch (error) { setError(error instanceof Error ? error.message : 'Unable to resolve this answer.'); }
    finally { setLoading(false); }
  }
  function change(value: string) {
    if (new TextEncoder().encode(value).length > 48000) { setError('Your answer exceeds the 48 KB limit.'); return; }
    textRef.current = value; dirtyRef.current = true; setContent(value); retain(value); setSaving('Unsaved changes');
  }
  return <main className="interview-shell">
    <header className="room-title"><div><p className="eyebrow">INTERVIEW IN PROGRESS</p><h1>{session.job_title}</h1></div><button className="button secondary" disabled={loading || finishing || conflict} onClick={() => setConfirm(true)}>Finish interview</button></header>
    <Proctoring token={token} sessionId={session.id} />
    {error && <p className="error" role="alert">{error}</p>}{localWarning && <p className="configuration-note">{localWarning}</p>}
    {remaining <= 0 ? <section className="panel empty"><h2>Time is up</h2><p>Confirming submission of your saved answers.</p><button className="button" disabled={finishing} onClick={() => void finish(true)}>{finishing ? 'Confirming…' : 'Retry confirmation'}</button></section> : <div className="simple-room">
      <section className="room-left">
        <div className="room-question">
          <div className="room-question-top"><span>Question {question?.position ?? '–'} / {session.question_count}</span><div className={`room-timer ${remaining <= 60000 ? 'urgent' : ''}`} role="timer" aria-label="Time remaining"><small>Time remaining</small><strong>{time(remaining)}</strong></div></div>
          {loading ? <p role="status">Loading question…</p> : question ? <h2 className="preserve">{question.prompt}</h2> : <button className="button" onClick={() => void load()}>Retry loading</button>}
          <div className="room-navigation" aria-label="Questions">{Array.from({ length: session.question_count }, (_, index) => <button key={index} className={question?.position === index + 1 ? 'current' : ''} aria-label={`Question ${index + 1}`} aria-current={question?.position === index + 1 ? 'step' : undefined} disabled={loading || finishing || conflict || posting} onClick={() => void navigate(index + 1)}>{index + 1}</button>)}<small>{session.answer_count} of {session.question_count} answered</small></div>
        </div>
        <InterviewMedia controlsTarget={mediaControls} />
      </section>
      <section className="room-right">
        <div className="room-answer-top"><span>Answer type <strong>{question?.kind === 'code' ? 'Code' : question?.kind === 'scenario' ? 'Scenario' : 'Text'}</strong></span><span>On this question <strong>{time(now - questionStarted)}</strong></span></div>
        <section className="room-submitted" aria-label="Submitted answer"><p className="eyebrow">YOUR SUBMITTED ANSWER</p>{submittedContent ? <pre className={question?.kind === 'code' ? 'submitted-code' : ''}>{submittedContent}</pre> : <p className="muted">Your answer will appear here when you click Submit answer.</p>}</section>
        <section className="room-composer"><div ref={setMediaControls} /><label htmlFor="room-answer">{question?.kind === 'code' ? 'Write your code' : 'Write your answer'}</label><textarea id="room-answer" aria-label={question?.kind === 'code' ? 'Your code' : 'Your answer'} className={question?.kind === 'code' ? 'code-editor' : ''} spellCheck={question?.kind !== 'code'} value={content} disabled={loading || finishing || posting || !question} onChange={event => change(event.target.value)} placeholder={question?.kind === 'code' ? '// Write your solution…' : 'Type your answer…'} /><div className="room-composer-footer"><span className="save-status" role="status">{saving}</span><button className="button" disabled={loading || finishing || conflict || posting || !question || !content.trim()} onClick={() => { setPosting(true); void flush().then(() => setSubmittedContent(textRef.current)).catch(() => {}).finally(() => setPosting(false)); }}>{posting ? 'Submitting…' : 'Submit answer ↑'}</button></div><small className="muted">Drafts save automatically. You can edit answers until you finish the interview.</small>{conflict && <div className="conflict-actions"><button className="button secondary" onClick={() => void resolve(false)}>Use saved answer</button><button className="button" onClick={() => void resolve(true)}>Save my draft instead</button></div>}</section>
      </section>
    </div>}
    <dialog ref={submitDialog} className="submit-dialog" onCancel={() => setConfirm(false)}><h2>Submit your interview?</h2><p>Your latest answer will be saved first. Submitted answers cannot be edited. You may submit with unanswered questions.</p><div className="actions"><button className="button secondary" disabled={finishing} onClick={() => setConfirm(false)}>Keep working</button><button className="button" disabled={finishing} onClick={() => { setConfirm(false); void finish(); }}>{finishing ? 'Submitting…' : 'Confirm submission'}</button></div></dialog>
  </main>;
}
