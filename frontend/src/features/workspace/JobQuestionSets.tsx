import { useEffect, useMemo, useState } from 'react';
import { workspaceApi, type JobPool } from './api';

export function JobQuestionSets({ job, token }: { job: string; token: string }) {
  const api = useMemo(() => workspaceApi(token), [token]);
  const [pool, setPool] = useState<JobPool | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const controller = new AbortController();
    async function load() {
      try {
        const next = await api.questionSets(job, controller.signal);
        if (!active) return;
        setPool(next); setError('');
        if (next.sets.some(set => ['pending', 'running'].includes(set.status))) {
          timer = setTimeout(() => void load(), 4000);
        }
      } catch (failure) {
        if (active) setError(failure instanceof Error ? failure.message : 'Unable to load question sets.');
      }
    }
    void load();
    return () => { active = false; controller.abort(); clearTimeout(timer); };
  }, [api, job, refresh]);
  async function retry() {
    setBusy(true); setError('');
    try { await api.retryQuestionSets(job); setRefresh(value => value + 1); }
    catch (failure) { setError(failure instanceof Error ? failure.message : 'Unable to retry preparation.'); }
    finally { setBusy(false); }
  }
  const unused = pool?.sets.filter(set => !set.assigned) ?? [];
  const counts = pool ? [
    `${unused.filter(set => set.status === 'completed').length} ready`,
    ...(['running', 'pending', 'failed'] as const).flatMap(status => {
      const count = unused.filter(set => set.status === status).length;
      return count ? [`${count} ${status === 'running' ? 'preparing' : status === 'pending' ? 'queued' : 'failed'}`] : [];
    }),
  ].join(' · ') : 'loading…';
  return <details className="job-question-sets">
    <summary>Question sets · {counts}</summary>
    {error && <p className="error" role="alert">{error} <button className="text-button" onClick={() => setRefresh(value => value + 1)}>Reload</button></p>}
    {pool && <>
      <button className="text-button" onClick={() => setRefresh(value => value + 1)}>Refresh sets</button>
      <p>Preparing {pool.target} unused sets for {pool.duration_minutes}-minute interviews. Each set is assigned once and replaced automatically.</p>
      {!pool.configured && <p className="configuration-note">AI is not configured. Queued sets will wait until the generation and embedding services are available.</p>}
      {unused.map((set, index) => <details key={set.id}>
        <summary>Set {index + 1} · {set.duration_minutes} min · {set.status} · {set.question_count} questions</summary>
        {set.last_error && <p className="error">{set.last_error}</p>}
        {pool.questions.filter(question => question.set_id === set.id).map(question => <details key={question.id}>
          <summary>Question {question.position} · {question.kind} · {question.difficulty}</summary>
          <p className="preserve">{question.prompt}</p>
          <h4>Reference answer</h4><p className="preserve">{question.rubric.expected_answer}</p>
          <h4>Scoring criteria</h4><ul>{question.rubric.criteria.map(criterion => <li key={criterion.id}>{criterion.max_points} points · {criterion.description}</li>)}</ul>
        </details>)}
      </details>)}
      {unused.some(set => set.status === 'failed') && <button className="button" disabled={busy || !pool.configured} onClick={() => void retry()}>{busy ? 'Queueing…' : 'Retry failed sets'}</button>}
      <p className="muted">Reference answers are visible only to the interviewer. A different invitation duration is prepared separately.</p>
    </>}
  </details>;
}
