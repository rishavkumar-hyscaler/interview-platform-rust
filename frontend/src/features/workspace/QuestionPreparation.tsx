import { Button } from '../../components/ui/button';
import { ExpandableSection } from './ExpandableSection';
import { useEffect, useMemo, useState } from 'react';
import { workspaceApi, type Generation, type Interview } from './api';

export function QuestionPreparation({ interview, token }: { interview: Interview; token: string }) {
  const api = useMemo(() => workspaceApi(token), [token]);
  const [generation, setGeneration] = useState<Generation | null>(null);
  const [error, setError] = useState('');
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
  const pending = generation?.status === 'pending' || generation?.status === 'running';
  return <section className="question-preparation" aria-label="Interview question preparation">
    <div className="preparation-heading"><h3>Interview questions & rubrics</h3>{generation && <span className={`status status-${generation.status}`}>{generation.status.replaceAll('_', ' ')}</span>}</div>
    {error && <p className="error" role="alert">{error}<Button variant="text" onClick={() => setRefresh(value => value + 1)}>Retry loading</Button></p>}
    {!generation && !error && <p role="status">Loading preparation status…</p>}
    {generation && <>
      {!generation.configured && generation.status !== 'completed' && <p className="configuration-note">AI setup is needed before questions can be prepared. The generation and embedding services must be configured first.</p>}
      {generation.status === 'not_started' && <p>Prepare focused 2–3 minute questions from this role’s job description, with reference answers and scoring rubrics. The question count is planned to fit the interview duration.</p>}
      {generation.questions.length > 0 && generation.status !== 'completed' && <p>{generation.questions.length} questions saved. Preparation will resume at the next unsaved question.</p>}
      {pending && <p role="status">{generation.status === 'running' ? 'Preparing one question at a time. Completed questions are saved below.' : 'Preparation queued. This page updates automatically.'}</p>}
      {generation.job?.last_error && <p className={generation.status === 'failed' ? 'error' : 'configuration-note'}>{generation.job.last_error}{pending && ' Retrying automatically.'}</p>}
      {generation.status !== 'completed' && <p>Manage preparation and retries under this job’s Question sets. Questions are prepared for the job and reused across candidates.</p>}
      {generation.blueprint && <ExpandableSection className="blueprint" label={<>Competency blueprint</>}>{generation.blueprint.competencies.map(c => <article key={c.id}><strong>{c.name} · {c.weight}%</strong><blockquote>{c.jd_evidence}</blockquote></article>)}</ExpandableSection>}
      {generation.status === 'completed' && <p className="success">{generation.questions.length} questions prepared. Rubrics below are visible to recruiters only.</p>}
      <div className="prepared-questions">{generation.questions.map(q => <ExpandableSection key={q.id} label={<>Question {q.position} · {q.kind} · {q.difficulty}</>}><p className="preserve">{q.prompt}</p><h4>Latest saved answer</h4><p className="preserve">{q.answer?.content || 'No answer saved.'}</p><h4>Worked reference answer</h4><p className="preserve">{q.rubric.expected_answer ?? 'No expected-answer guidance stored.'}</p>{q.rubric.estimated_minutes && <p>Estimated task time: {q.rubric.estimated_minutes} minutes</p>}{q.rubric.quality_review?.reference_correct && <p className="success">Passed structural checks and a separate AI quality review.</p>}<p className="muted">This is an illustrative reference, not the only acceptable wording or approach.</p>{q.rubric.assumptions?.length ? <><h4>Assumptions</h4><ul>{q.rubric.assumptions.map((text, i) => <li key={i}>{text}</li>)}</ul></> : null}{q.rubric.acceptable_alternatives?.length ? <><h4>Acceptable alternatives</h4><ul>{q.rubric.acceptable_alternatives.map((text, i) => <li key={i}>{text}</li>)}</ul></> : null}{q.rubric.common_mistakes?.length ? <><h4>Common mistakes</h4><ul>{q.rubric.common_mistakes.map((text, i) => <li key={i}>{text}</li>)}</ul></> : null}{q.rubric.coding && <><h4>Reference solution · {q.rubric.coding.language}</h4><pre>{q.rubric.coding.solution}</pre><p>{q.rubric.coding.complexity}</p><p className="muted">The solution and examples are reference material; they have not been executed by the platform.</p>{q.rubric.coding.tests.map((test, i) => <article key={i}><strong>{test.category} case</strong><p>Input</p><pre>{test.input}</pre><p>Expected output</p><pre>{test.expected_output}</pre><p>{test.explanation}</p></article>)}</>}<h4>Scoring rubric · {q.rubric.max_score ?? 10} points</h4><ul>{(q.rubric.criteria ?? []).map(c => <li key={c.id}><strong>{c.max_points} points:</strong> {c.description}{c.partial_credit && <p>Partial credit: {c.partial_credit}</p>}{c.full_credit && <p>Full credit: {c.full_credit}</p>}{c.reference_evidence && <blockquote>{c.reference_evidence}{c.reference_source && <small> · {c.reference_source.startsWith('reference_sections.') ? 'Criterion reference section' : c.reference_source === 'coding.solution' ? 'Reference solution' : c.reference_source === 'coding.complexity' ? 'Complexity analysis' : 'Worked answer'}</small>}</blockquote>}</li>)}</ul></ExpandableSection>)}</div>
      {generation.job && <p className="generation-meta">Attempt {generation.job.attempts} of {generation.max_attempts ?? 4} · {generation.prompt_version}</p>}
    </>}
  </section>;
}
