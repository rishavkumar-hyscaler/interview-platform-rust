import { useEffect, useMemo, useState } from 'react';
import { workspaceApi, type Generation, type Interview } from './api';

export function QuestionPreparation({ interview, token }: { interview: Interview; token: string }) {
  const api = useMemo(() => workspaceApi(token), [token]);
  const [generation, setGeneration] = useState<Generation | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const controller = new AbortController();
    async function poll() {
      try {
        const next = await api.generation(interview.id, controller.signal);
        if (!active) return;
        setGeneration(next); setError('');
        if (next.status === 'pending' || next.status === 'running') timer = setTimeout(() => void poll(), 2000);
      } catch (error) {
        if (active) setError(error instanceof Error ? error.message : 'Unable to load question preparation.');
      }
    }
    void poll();
    return () => { active = false; controller.abort(); clearTimeout(timer); };
  }, [api, interview.id, refresh]);
  async function prepare() {
    setBusy(true); setError('');
    try { await api.prepare(interview.id); setRefresh(value => value + 1); }
    catch (error) { setError(error instanceof Error ? error.message : 'Unable to queue question preparation.'); }
    finally { setBusy(false); }
  }
  const pending = generation?.status === 'pending' || generation?.status === 'running';
  const eligible = ['invited', 'ready'].includes(interview.state) && Date.parse(interview.expires_at) > Date.now();
  return <section className="question-preparation" aria-label="Interview question preparation">
    <div className="preparation-heading"><h3>Interview questions & rubrics</h3>{generation && <span className={`status status-${generation.status}`}>{generation.status.replaceAll('_', ' ')}</span>}</div>
    {error && <p className="error" role="alert">{error}<button className="text-button" onClick={() => setRefresh(value => value + 1)}>Retry loading</button></p>}
    {!generation && !error && <p role="status">Loading preparation status…</p>}
    {generation && <>
      {!generation.configured && generation.status !== 'completed' && <p className="configuration-note">AI setup is needed before questions can be prepared. The generation and embedding services must be configured first.</p>}
      {generation.status === 'not_started' && <p>Prepare six unique questions from this role’s job description, with competency coverage and a scoring rubric for every question.</p>}
      {pending && <p role="status">{generation.status === 'running' ? 'Generating questions and checking them against the job’s question bank…' : 'Preparation queued. This page updates automatically.'}</p>}
      {generation.job?.last_error && <p className={generation.status === 'failed' ? 'error' : 'configuration-note'}>{generation.job.last_error}{pending && ' Retrying automatically.'}</p>}
      {generation.status !== 'completed' && eligible && <button className="button" disabled={busy || pending || !generation.configured} onClick={() => void prepare()}>{busy ? 'Queueing…' : pending ? 'Preparing questions…' : generation.status === 'failed' ? 'Retry preparation' : 'Prepare interview questions'}</button>}
      {!eligible && generation.status !== 'completed' && <p>Question preparation is available before an active interview starts.</p>}
      {generation.blueprint && <details className="blueprint"><summary>Competency blueprint</summary>{generation.blueprint.competencies.map(c => <article key={c.id}><strong>{c.name} · {c.weight}%</strong><blockquote>{c.jd_evidence}</blockquote></article>)}</details>}
      {generation.status === 'completed' && <p className="success">{generation.questions.length} questions prepared. Rubrics below are visible to recruiters only.</p>}
      <div className="prepared-questions">{generation.questions.map(q => <details key={q.id}><summary>Question {q.position} · {q.kind} · {q.difficulty}</summary><p className="preserve">{q.prompt}</p><h4>Latest saved answer</h4><p className="preserve">{q.answer?.content || 'No answer saved.'}</p><h4>Worked reference answer</h4><p className="preserve">{q.rubric.expected_answer ?? 'No expected-answer guidance stored.'}</p>{q.rubric.estimated_minutes && <p>Estimated task time: {q.rubric.estimated_minutes} minutes</p>}{q.rubric.quality_review?.reference_correct && <p className="success">Passed structural checks and a separate AI quality review.</p>}<p className="muted">This is an illustrative reference, not the only acceptable wording or approach.</p>{q.rubric.assumptions?.length ? <><h4>Assumptions</h4><ul>{q.rubric.assumptions.map((text, i) => <li key={i}>{text}</li>)}</ul></> : null}{q.rubric.acceptable_alternatives?.length ? <><h4>Acceptable alternatives</h4><ul>{q.rubric.acceptable_alternatives.map((text, i) => <li key={i}>{text}</li>)}</ul></> : null}{q.rubric.common_mistakes?.length ? <><h4>Common mistakes</h4><ul>{q.rubric.common_mistakes.map((text, i) => <li key={i}>{text}</li>)}</ul></> : null}{q.rubric.coding && <><h4>Reference solution · {q.rubric.coding.language}</h4><pre>{q.rubric.coding.solution}</pre><p>{q.rubric.coding.complexity}</p><p className="muted">The solution and examples are reference material; they have not been executed by the platform.</p>{q.rubric.coding.tests.map((test, i) => <article key={i}><strong>{test.category} case</strong><p>Input</p><pre>{test.input}</pre><p>Expected output</p><pre>{test.expected_output}</pre><p>{test.explanation}</p></article>)}</>}<h4>Scoring rubric · {q.rubric.max_score ?? 10} points</h4><ul>{(q.rubric.criteria ?? []).map(c => <li key={c.id}><strong>{c.max_points} points:</strong> {c.description}{c.partial_credit && <p>Partial credit: {c.partial_credit}</p>}{c.full_credit && <p>Full credit: {c.full_credit}</p>}{c.reference_evidence && <blockquote>{c.reference_evidence}{c.reference_source && <small> · {c.reference_source.startsWith('reference_sections.') ? 'Criterion reference section' : c.reference_source === 'coding.solution' ? 'Reference solution' : c.reference_source === 'coding.complexity' ? 'Complexity analysis' : 'Worked answer'}</small>}</blockquote>}</li>)}</ul></details>)}</div>
      {generation.job && <p className="generation-meta">Attempt {generation.job.attempts} of 3 · {generation.prompt_version}</p>}
    </>}
  </section>;
}
