import { useEffect, useMemo, useState } from 'react';
import { workspaceApi, type JobPool, type PreparationFailure } from './api';

function FailureDetails({ failure }: { failure: PreparationFailure }) {
  const checks = failure.findings?.questions ?? [];
  const labels = { jd_grounded: 'Job relevance needs revision', reference_correct: 'Reference correctness needs revision', rubric_aligned: 'Scoring criteria need revision', feasible: 'Task scope or time needs revision', unambiguous: 'Question needs clarification', no_answer_leak: 'Question reveals answer guidance' } as const;
  const issues = checks.map(check => ({ position: failure.question_position ?? check.position, messages: [...new Set([...(check.concerns ?? []), ...Object.entries(labels).filter(([key]) => check[key as keyof typeof labels] === false).map(([, label]) => label)])] })).filter(check => check.messages.length);
  if (!issues.length) return null;
  return <div className="preparation-findings"><strong>{failure.phase === 'reference' ? 'Reference review findings' : 'Question review findings'}</strong><ul>{issues.map((issue, index) => <li key={index}>Question {issue.position ?? '—'}: {issue.messages.join('; ')}</li>)}</ul></div>;
}

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
          timer = setTimeout(() => void load(), 10000);
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
      <p>Configured for {pool.target} sets for {pool.duration_minutes}-minute interviews. Each question is validated and saved before the next begins. Prepared sets are reused across candidates, with separate answers for each candidate. No automatic replacements.</p>
      {!pool.configured && <p className="configuration-note">AI is not configured. Queued sets will wait until the generation and embedding services are available.</p>}
      {unused.map((set, index) => <details key={set.id}>
        <summary>Set {index + 1} · {set.duration_minutes} min · {set.status} · {set.question_count} saved questions</summary>
        {set.last_error && <p className="error">{set.last_error}</p>}
        {set.status === 'failed' && set.last_failure && <FailureDetails failure={set.last_failure} />}
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
