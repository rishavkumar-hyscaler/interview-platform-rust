import { Button } from '../../components/ui/button';
import { ExpandableSection } from './ExpandableSection';
import { HelpTip } from './HelpTip';
import { BrowserObservations } from './BrowserObservations';
import { useEffect, useState } from 'react';
import { request } from '../../api';
import { loggedFetch } from '../../transport';
import { startTask } from '../../observability';
type Criterion = {
  criterion_id: string;
  points: number;
  evidence: string;
  reasoning: string;
};
type LiveCriterion = Partial<Criterion>;
type ScoredQuestion = {
  question_id: string;
  position: number;
  competency_id: string;
  score: number | null;
  max_score: number;
  criteria: Criterion[];
};
type LiveEvent =
  | {
      type: 'rationale';
      position: number;
      criteria: LiveCriterion[];
    }
  | { type: 'scored'; position: number; result: ScoredQuestion }
  | { type: 'rejected'; position: number }
  | { type: 'finished'; success: boolean };
type Report = {
  configured: boolean;
  state: string;
  failed: boolean;
  attempts: number;
  max_attempts?: number;
  last_failure?: { reason: string; question_position?: number | null } | null;
  assessment: null | {
    model: string;
    competency_profile?: {
      weighted_percent: number | null;
      recommendation: string;
      competencies: {
        id: string;
        name: string;
        percent: number | null;
        weight: number;
      }[];
    };
    questions: {
      question_id: string;
      position: number;
      competency_id: string;
      score: number | null;
      status?: string;
      max_score: number;
      criteria: Criterion[];
    }[];
  };
  observations: {
    id: string;
    received_at: string;
    observation: {
      kind: string;
      character_count?: number | null;
      measurement?: number | null;
      unit?: string;
      measurements?: Record<string, number | null>;
    };
  }[];
  integrity_notice: string;
};
export function Assessment({
  session,
  token,
}: {
  session: string;
  token: string;
}) {
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  const [retrying, setRetrying] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const [drafting, setDrafting] = useState<{
    position: number;
    criteria: LiveCriterion[];
  } | null>(null);
  const [scored, setScored] = useState<ScoredQuestion[]>([]);
  const scoring =
    !!report &&
    report.configured &&
    !report.assessment &&
    !report.failed &&
    ['submitted', 'scoring'].includes(report.state);
  useEffect(() => {
    const controller = new AbortController();
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await request<Report>(
          `/api/v1/sessions/${session}/assessment`,
          { signal: controller.signal },
          token,
        );
        if (!active) return;
        setReport(next);
        setError('');
        if (!next.assessment && !next.failed)
          timer = setTimeout(() => void poll(), 5000);
      } catch (error) {
        if (active)
          setError(
            error instanceof Error
              ? error.message
              : 'Unable to load assessment',
          );
      }
    }
    void poll();
    return () => {
      active = false;
      controller.abort();
      clearTimeout(timer);
    };
  }, [session, token, retry, refresh]);
  // Rationale streams while each answer is graded. It is unverified until the
  // server validates the evidence quotes; the polled report stays authoritative.
  useEffect(() => {
    if (!scoring) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function connect() {
      const requestId = crypto.randomUUID();
      const finishTask = startTask('assessment.stream', requestId);
      try {
        const response = await loggedFetch(
          `/api/v1/sessions/${session}/assessment/stream`,
          {
            headers: {
              Authorization: `Bearer ${token}`,
              'X-Request-ID': requestId,
            },
            signal: controller.signal,
          },
        );
        if (!response.ok || !response.body)
          throw new Error('Live scoring unavailable');
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';
        while (!controller.signal.aborted) {
          const { value, done } = await reader.read();
          if (done) break;
          buffer += decoder
            .decode(value, { stream: true })
            .replaceAll('\r\n', '\n');
          let end: number;
          while ((end = buffer.indexOf('\n\n')) >= 0) {
            const block = buffer.slice(0, end);
            buffer = buffer.slice(end + 2);
            const data = block
              .split('\n')
              .filter((line) => line.startsWith('data:'))
              .map((line) => line.slice(5).trimStart())
              .join('\n');
            if (!data) continue;
            const event = JSON.parse(data) as LiveEvent;
            if (event.type === 'rationale')
              setDrafting({
                position: event.position,
                criteria: event.criteria,
              });
            else if (event.type === 'scored') {
              setDrafting(null);
              setScored((previous) => [
                ...previous.filter((q) => q.position !== event.position),
                event.result,
              ]);
            } else if (event.type === 'rejected') setDrafting(null);
            else {
              setDrafting(null);
              setRefresh((value) => value + 1);
            }
          }
        }
        finishTask(controller.signal.aborted ? 'cancelled' : 'succeeded');
      } catch {
        finishTask(controller.signal.aborted ? 'cancelled' : 'failed');
      }
      if (!controller.signal.aborted)
        timer = setTimeout(() => void connect(), 3000);
    }
    void connect();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [scoring, session, token]);
  async function retryScoring() {
    if (retrying) return;
    setRetrying(true);
    setError('');
    try {
      await request(
        `/api/v1/sessions/${session}/assessment/retry`,
        { method: 'POST' },
        token,
      );
      setReport((previous) =>
        previous
          ? {
              ...previous,
              failed: false,
              attempts: 0,
              last_failure: null,
              state: 'submitted',
            }
          : previous,
      );
      setRetry((value) => value + 1);
    } catch (failure) {
      setError(
        failure instanceof Error ? failure.message : 'Unable to retry scoring',
      );
    } finally {
      setRetrying(false);
    }
  }
  async function download() {
    let exported: unknown;
    try {
      exported = await request(`/api/v1/sessions/${session}/report`, {}, token);
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Export failed');
      return;
    }
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(exported, null, 2)], {
        type: 'application/json',
      }),
    );
    const link = document.createElement('a');
    link.href = url;
    link.download = `assessment-${session}.json`;
    link.click();
    URL.revokeObjectURL(url);
  }
  return (
    <section className="question-preparation">
      <div className="preparation-heading">
        <h3>Assessment & integrity evidence</h3>
        {report && (
          <Button variant="secondary" onClick={() => void download()}>
            Export JSON
          </Button>
        )}
      </div>
      {error && (
        <p role="alert" className="error">
          {error}
          <Button variant="text" onClick={() => setRetry((value) => value + 1)}>
            Retry
          </Button>
        </p>
      )}
      {!report && !error && <p role="status">Loading assessment…</p>}
      {report && (
        <>
          {report.assessment ? (
            <>
              <p className="success">
                Rubric score:{' '}
                {report.assessment.questions.reduce(
                  (sum, q) => sum + (q.score ?? 0),
                  0,
                )}{' '}
                /{' '}
                {report.assessment.questions.reduce(
                  (sum, q) => sum + (q.score == null ? 0 : q.max_score),
                  0,
                )}{' '}
                · Human review required
              </p>
              {report.assessment.questions.map((q) => (
                <ExpandableSection
                  key={q.question_id}
                  label={
                    <>
                      Question {q.position} · {q.competency_id} ·{' '}
                      {q.score ?? 'Needs human review'}/{q.max_score}
                    </>
                  }
                >
                  {q.criteria.map((c) => (
                    <article key={c.criterion_id}>
                      <strong>
                        {c.criterion_id}: {c.points} points
                      </strong>
                      {c.evidence && (
                        <blockquote className="preserve">
                          {c.evidence}
                        </blockquote>
                      )}
                      <p>{c.reasoning}</p>
                    </article>
                  ))}
                </ExpandableSection>
              ))}
              {report.assessment.competency_profile && (
                <>
                  <h4>Competency profile</h4>
                  <p>{report.assessment.competency_profile.recommendation}</p>
                  {report.assessment.competency_profile.competencies.map(
                    (c) => (
                      <p key={c.id}>
                        {c.name}:{' '}
                        {c.percent == null
                          ? 'No evidence'
                          : `${Math.round(c.percent)}%`}{' '}
                        · JD weight {c.weight}%
                      </p>
                    ),
                  )}
                </>
              )}
              {report.assessment.questions.some((q) => q.score == null) && (
                <p className="configuration-note">
                  Media-only answers need human review and are excluded from the
                  numeric AI total.
                </p>
              )}
              <small>Scoring model: {report.assessment.model}</small>
            </>
          ) : (
            <p role="status">
              {!report.configured
                ? 'AI scoring is not configured on this server. Configure the AI keys and restart the backend.'
                : report.failed
                  ? 'Scoring needs attention after the initial attempt and three retries.'
                  : ['submitted', 'scoring'].includes(report.state)
                    ? 'Scoring saved answers against the stored rubrics…'
                    : 'Scoring begins after the interview is submitted.'}
            </p>
          )}
          {scoring && (scored.length > 0 || drafting) && (
            <div className="live-scoring" aria-live="polite">
              <h4>
                Live scoring{' '}
                <HelpTip text="Rationale appears while the model writes it. Points only count after the server checks every evidence quote against the answer; the final assessment replaces this view." />
              </h4>
              {[...scored]
                .sort((a, b) => a.position - b.position)
                .map((q) => (
                  <p key={q.question_id}>
                    Question {q.position} · {q.competency_id} ·{' '}
                    {q.score ?? 'Needs human review'}/{q.max_score} · evidence
                    checked
                  </p>
                ))}
              {drafting && (
                <article>
                  <strong>
                    Question {drafting.position} · writing rationale
                    (unverified)…
                  </strong>
                  {drafting.criteria.map((c, index) => (
                    <div key={c.criterion_id ?? index}>
                      <p>
                        <em>{c.criterion_id ?? 'criterion'}</em>
                        {c.points != null && ` · ${c.points} points proposed`}
                      </p>
                      {c.reasoning && <p className="preserve">{c.reasoning}</p>}
                      {c.evidence && (
                        <blockquote className="preserve">
                          {c.evidence}
                        </blockquote>
                      )}
                    </div>
                  ))}
                </article>
              )}
            </div>
          )}
          {!report.assessment && report.last_failure && (
            <p className="configuration-note">
              Last scoring failure
              {report.last_failure.question_position
                ? ` at question ${report.last_failure.question_position}`
                : ''}
              : {report.last_failure.reason}. Attempt {report.attempts}/
              {report.max_attempts ?? 4}
              {!report.failed && ' · automatic retry pending'}.
            </p>
          )}
          {report.failed && (
            <div className="configuration-note">
              <p>
                Automatic scoring retries are exhausted. You can manually start
                a new retry cycle.
              </p>
              <Button
                type="button"
                variant="secondary"
                disabled={retrying || !report.configured}
                onClick={() => void retryScoring()}
              >
                {retrying ? 'Queuing…' : 'Retry scoring manually'}
              </Button>
            </div>
          )}
          <h4>
            Candidate observations ({report.observations.length}){' '}
            <HelpTip text="Saved browser, device, face and voice observations for this session. Unlike recent events, this list is not limited to the latest 30 live updates." />
          </h4>
          <p className="muted">{report.integrity_notice}</p>
          {report.observations.length === 0 ? (
            <p>
              No browser observations recorded. This does not establish that no
              suspicious activity occurred.
            </p>
          ) : (
            <BrowserObservations events={report.observations} />
          )}
        </>
      )}
    </section>
  );
}
