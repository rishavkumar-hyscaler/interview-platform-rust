import { RefreshButton } from '../../components/ui/refresh-button';
import { ExpandableSection } from './ExpandableSection';
import { useEffect, useMemo, useState } from 'react';
import { workspaceApi, type JobPool, type PreparationFailure } from './api';

function FailureDetails({ failure }: { failure: PreparationFailure }) {
  const checks = failure.findings?.questions ?? [];
  const labels = { jd_grounded: 'Job relevance needs revision', reference_correct: 'Reference correctness needs revision', rubric_aligned: 'Scoring criteria need revision', feasible: 'Task scope or time needs revision', unambiguous: 'Question needs clarification', no_answer_leak: 'Question reveals answer guidance' } as const;
  const issues = checks.map(check => ({ position: failure.question_position ?? check.position, messages: [...new Set([...(check.concerns ?? []), ...Object.entries(labels).filter(([key]) => check[key as keyof typeof labels] === false).map(([, label]) => label)])] })).filter(check => check.messages.length);
  if (!issues.length) return null;
  return <div className="preparation-findings"><strong>{failure.phase === 'reference' ? 'Reference review findings' : 'Question review findings'}</strong><ul>{issues.map((issue, index) => <li key={index}>Question {issue.position ?? '—'}: {issue.messages.join('; ')}</li>)}</ul></div>;
}

export function JobQuestionSets({ job, token, onReadyChange, summaryOnly = false }: { summaryOnly?: boolean; job: string; token: string; onReadyChange?: (job: string, durations: number[]) => void }) {
  const api = useMemo(() => workspaceApi(token), [token]);
  const [pool, setPool] = useState<JobPool | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [refresh, setRefresh] = useState(0);
  function reload() {
    setRefreshing(true);
    setRefresh(value => value + 1);
  }
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const controller = new AbortController();
    async function load() {
      try {
        const next = await api.questionSets(job, controller.signal);
        if (!active) return;
        setPool(next); setError('');
        onReadyChange?.(job, [...new Set(next.sets.filter(set => set.status === 'completed' && set.question_count > 0).map(set => set.duration_minutes))]);
        if (next.sets.some(set => ['pending', 'running'].includes(set.status))) {
          timer = setTimeout(() => void load(), 10000);
        }
      } catch (failure) {
        if (active) onReadyChange?.(job, []);
        if (active) setError(failure instanceof Error ? failure.message : 'Unable to load question sets.');
      } finally {
        if (active) setRefreshing(false);
      }
    }
    void load();
    return () => { active = false; controller.abort(); clearTimeout(timer); };
  }, [api, job, refresh, onReadyChange]);
  async function retry() {
    if (busy) return;
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
  if (summaryOnly) {
    return <span className="text-detail text-slate-600 workspace-dark:text-slate-300">
      Question sets · {error ? 'Status unavailable' : counts}
    </span>;
  }
  return <ExpandableSection className="job-question-sets" label={<>Question sets · {counts}</>}>

    {error && <p className="error" role="alert">{error} <RefreshButton pending={refreshing} disabled={busy} onClick={reload}>Reload</RefreshButton></p>}
    {pool && <>
      <RefreshButton pending={refreshing} disabled={busy} onClick={reload}>Refresh sets</RefreshButton>
      <p>Configured for {pool.target} sets for {pool.duration_minutes}-minute interviews. Each question is validated and saved before the next begins. Prepared sets are reused across candidates, with separate answers for each candidate. No automatic replacements.</p>
      {!pool.configured && <p className="configuration-note">AI is not configured. Queued sets will wait until the generation and embedding services are available.</p>}
      <div className="question-set-list">
      {unused.map((set, index) => <ExpandableSection key={set.id} className="question-set-card" label={<span className="set-card-heading"><span className="set-card-title"><span className="set-number">{String(index + 1).padStart(2, '0')}</span><span>Set {index + 1}</span><span className={`status status-${set.status}`}>{set.status.replaceAll('_', ' ')}</span></span><span className="set-card-meta">{set.duration_minutes} min · {set.question_count} saved questions{set.attempts > 0 && ` · attempt ${set.attempts}/${pool.max_attempts ?? 4}`}</span></span>}>
        <div className="set-card-body" role="region" aria-label={`Set ${index + 1} questions`} tabIndex={0}>

        {set.plan && <><p>{set.plan.allocations.length} planned questions · {set.plan.allocations.reduce((sum, slot) => sum + slot.minutes, 0)} minutes of tasks · {set.plan.duration_minutes - set.plan.allocations.reduce((sum, slot) => sum + slot.minutes, 0)} minutes for review</p><ExpandableSection label={<>Question time plan</>}><ol>{set.plan.allocations.map((slot, index) => <li key={index}>{slot.minutes} min · {slot.scope}</li>)}</ol></ExpandableSection></>}
        {set.last_error && <p className="error">{set.last_error}</p>}
        {set.status === 'failed' && set.last_failure && <FailureDetails failure={set.last_failure} />}
        {pool.questions.filter(question => question.set_id === set.id).map(question => <ExpandableSection key={question.id} label={<>Question {question.position} · {question.kind} · {question.difficulty}</>}>

          <p className="preserve">{question.prompt}</p>
          <h4>Reference answer</h4><p className="preserve">{question.rubric.expected_answer}</p>
          <h4>Scoring criteria</h4><ul>{question.rubric.criteria.map(criterion => <li key={criterion.id}>{criterion.max_points} points · {criterion.description}</li>)}</ul>
        </ExpandableSection>)}
        </div>
      </ExpandableSection>)}
      </div>
      {unused.some(set => set.status === 'failed') && <button className="button" disabled={busy || !pool.configured} onClick={() => void retry()}>{busy ? 'Queueing…' : 'Retry failed sets manually'}</button>}
      <p className="muted">Reference answers are visible only to the interviewer. A different invitation duration is prepared separately.</p>
    </>}
  </ExpandableSection>;
}
