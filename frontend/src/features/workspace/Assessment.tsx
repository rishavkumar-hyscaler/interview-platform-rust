import { useEffect, useState } from 'react';
import { request } from '../../api';
type Criterion = { criterion_id: string; points: number; evidence: string; reasoning: string };
type Report = { configured: boolean; state: string; failed: boolean; attempts: number; assessment: null | { model: string; competency_profile?: { weighted_percent: number | null; recommendation: string; competencies: { id: string; name: string; percent: number | null; weight: number }[] }; questions: { question_id: string; position: number; competency_id: string; score: number | null; status?: string; max_score: number; criteria: Criterion[] }[] }; observations: { id: string; received_at: string; observation: { kind: string; character_count?: number } }[]; integrity_notice: string };
export function Assessment({ session, token }: { session: string; token: string }) {
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    const controller = new AbortController(); let active = true; let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await request<Report>(`/api/v1/sessions/${session}/assessment`, { signal: controller.signal }, token);
        if (!active) return;
        setReport(next); setError('');
        if (!next.assessment && !next.failed) timer = setTimeout(() => void poll(), 5000);
      } catch (error) { if (active) setError(error instanceof Error ? error.message : 'Unable to load assessment'); }
    }
    void poll(); return () => { active = false; controller.abort(); clearTimeout(timer); };
  }, [session, token, retry]);
  async function download() {
    let exported: unknown;
    try { exported = await request(`/api/v1/sessions/${session}/report`, {}, token); } catch (error) { setError(error instanceof Error ? error.message : "Export failed"); return; }
    const url = URL.createObjectURL(new Blob([JSON.stringify(exported, null, 2)], { type: 'application/json' }));
    const link = document.createElement('a'); link.href = url; link.download = `assessment-${session}.json`; link.click(); URL.revokeObjectURL(url);
  }
  return <section className="question-preparation"><div className="preparation-heading"><h3>Assessment & integrity evidence</h3>{report && <button className="button secondary" onClick={() => void download()}>Export JSON</button>}</div>{error && <p role="alert" className="error">{error}<button className="text-button" onClick={() => setRetry(value => value + 1)}>Retry</button></p>}{!report && !error && <p role="status">Loading assessment…</p>}{report && <>{report.assessment ? <><p className="success">Rubric score: {report.assessment.questions.reduce((sum, q) => sum + (q.score ?? 0), 0)} / {report.assessment.questions.reduce((sum, q) => sum + (q.score == null ? 0 : q.max_score), 0)} · Human review required</p>{report.assessment.questions.map(q => <details key={q.question_id}><summary>Question {q.position} · {q.competency_id} · {q.score ?? 'Needs human review'}/{q.max_score}</summary>{q.criteria.map(c => <article key={c.criterion_id}><strong>{c.criterion_id}: {c.points} points</strong>{c.evidence && <blockquote className="preserve">{c.evidence}</blockquote>}<p>{c.reasoning}</p></article>)}</details>)}{report.assessment.competency_profile && <><h4>Competency profile</h4><p>{report.assessment.competency_profile.recommendation}</p>{report.assessment.competency_profile.competencies.map(c => <p key={c.id}>{c.name}: {c.percent == null ? 'No evidence' : `${Math.round(c.percent)}%`} · JD weight {c.weight}%</p>)}</>}{report.assessment.questions.some(q => q.score == null) && <p className="configuration-note">Media-only answers need human review and are excluded from the numeric AI total.</p>}<small>Scoring model: {report.assessment.model}</small></> : <p role="status">{!report.configured ? 'AI scoring is not configured on this server. Configure the AI keys and restart the backend.' : report.failed ? 'Scoring needs attention after three unsuccessful attempts.' : ['submitted', 'scoring'].includes(report.state) ? 'Scoring saved answers against the stored rubrics…' : 'Scoring begins after the interview is submitted.'}</p>}{report.failed && <button className="button secondary" onClick={() => void request(`/api/v1/sessions/${session}/assessment/retry`, { method: 'POST' }, token).then(() => setRetry(v => v + 1)).catch(error => setError(error instanceof Error ? error.message : 'Retry failed'))}>Retry scoring</button>}<h4>Browser observations ({report.observations.length})</h4><p className="muted">{report.integrity_notice}</p>{report.observations.length === 0 ? <p>No browser observations recorded. This does not establish that no suspicious activity occurred.</p> : <details><summary>View evidence</summary>{report.observations.map(event => <p key={event.id}><strong>{event.observation.kind.replaceAll('_', ' ')}</strong> · {new Date(event.received_at).toLocaleString()}{event.observation.character_count != null && ` · ${event.observation.character_count} characters`}<br /><code>{event.id}</code></p>)}</details>}</>}</section>;
}
