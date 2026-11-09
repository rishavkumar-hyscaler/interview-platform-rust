import { useEffect, useState } from 'react';
import { request } from '../../api';
import { HelpTip } from './HelpTip';

type ScoreGroup = {
  question_id: string;
  set_number?: number;
  position?: number;
  prompt: string;
  kind: string;
  difficulty: string;
  competency_id: string;
  sample_size: number;
  attempt_count?: number;
  comparison_count: number;
  average_seconds: number | null;
  timing_count: number;
  mean_points: number | null;
  item_total_correlation: number | null;
};

type Stats = { groups: ScoreGroup[] };

function readableLabel(value: string) {
  return value.replace(/[_-]+/g, ' ').replace(/\b\w+/g, word => {
    const acronyms: Record<string, string> = { api: 'API', apis: 'APIs', db: 'DB', jwt: 'JWT', rbac: 'RBAC', html: 'HTML', css: 'CSS' };
    return acronyms[word.toLowerCase()] ?? word.charAt(0).toUpperCase() + word.slice(1);
  });
}

const questionTypes: Record<string, string> = {
  text: 'Written response',
  code: 'Coding exercise',
  scenario: 'Scenario',
};

const ratingSampleSize = 20;
const questionPerformanceHelp = `For each candidate, we divide the question score by its maximum and the points on the remaining questions by their combined maximum. The chart shows the Pearson correlation between those two percentages across candidates who received the same question. Ratings appear after ${ratingSampleSize} comparable interviews. These descriptive results depend on the candidate sample and the other questions in each set.`;

function separation(group: ScoreGroup) {
  if (group.comparison_count === 0) return 'No paired scores yet';
  if (group.comparison_count === 1) return 'Correlation requires at least two comparable interviews';
  if (group.item_total_correlation == null) return 'Scores did not vary enough for correlation';
  if (group.comparison_count < ratingSampleSize) return 'Early result · small sample';
  if (group.item_total_correlation >= 0.3) return 'Separates stronger and weaker scores';
  if (group.item_total_correlation <= -0.2) return 'Reverse pattern · review question';
  if (Math.abs(group.item_total_correlation) < 0.2) return 'Little separation · review question';
  return 'Some separation';
}

function duration(seconds: number) {
  const rounded = Math.round(seconds);
  return `${Math.floor(rounded / 60)}m ${rounded % 60}s`;
}

function questionTooltip(group: ScoreGroup) {
  const average = group.mean_points == null ? 'Awaiting scores' : `${Number(group.mean_points).toFixed(1)}/10 average rubric score`;
  return [
    group.prompt,
    separation(group),
    `${group.timing_count} interviews with recorded timing`,
    `${readableLabel(group.competency_id)} · ${questionTypes[group.kind] ?? readableLabel(group.kind)} · ${readableLabel(group.difficulty)}`,
    `${average} · ${group.sample_size} scored responses`,
    `${group.attempt_count ?? group.timing_count} submitted interviews opened this question · ${group.comparison_count} comparable scores`,
  ].join('\n\n');
}

function questionLabel(group: ScoreGroup, index: number) {
  return group.set_number != null && group.position != null
    ? `Set ${group.set_number} · Q${group.position}`
    : `Q${index + 1}`;
}

function QuestionBarChart({ groups, title, help, value, format, maximum }: {
  groups: ScoreGroup[];
  title: string;
  help: string;
  value: (group: ScoreGroup) => number | null;
  format: (value: number) => string;
  maximum: number;
}) {
  return <figure className="m-0 rounded-lg border border-slate-200 p-3 workspace-dark:border-slate-700">
    <figcaption className="mb-3 flex items-center justify-between gap-2 font-semibold">
      {title}
      <HelpTip text={help} />
    </figcaption>
    <ol className="m-0 grid list-none gap-3 p-0">
      {groups.map((group, index) => {
        const amount = value(group);
        return <li key={group.question_id}>
          <div className="flex min-h-6 items-center justify-between gap-3 text-sm">
            <span>{questionLabel(group, index)} <HelpTip text={questionTooltip(group)} /></span>
            <span className="text-right tabular-nums">{amount === null ? 'Unavailable' : format(amount)}</span>
          </div>
          <div aria-hidden="true" className="mt-1 h-1.5 overflow-hidden rounded bg-slate-200 workspace-dark:bg-slate-700">
            {amount !== null && <div className="h-full rounded bg-indigo-500" style={{ width: `${100 * amount / maximum}%` }} />}
          </div>
        </li>;
      })}
    </ol>
  </figure>;
}

const attempts = (group: ScoreGroup) => group.attempt_count ?? group.timing_count;

function QuestionCharts({ groups }: { groups: ScoreGroup[] }) {
  const maxSeconds = Math.max(1, ...groups.map(group => Number(group.average_seconds ?? 0)));

  return <div className="my-4 grid items-start gap-4 lg:grid-cols-2">
    <figure className="m-0 rounded-lg border border-slate-200 p-3 workspace-dark:border-slate-700">
      <figcaption className="mb-3 flex items-center justify-between gap-2 font-semibold">
        Score separation
        <HelpTip text={`${questionPerformanceHelp} Ratings describe rubric performance. A value closer to +1 means the question better separates candidates who score higher on the rest of the interview from those who score lower. A value near 0 shows little linear separation. Negative values show a reverse pattern.`} />
      </figcaption>
      <ol className="m-0 grid list-none gap-3 p-0">
        {groups.map((group, index) => {
          const score = group.item_total_correlation != null
            ? Number(group.item_total_correlation) : null;
          return <li key={group.question_id}>
            <div className="flex min-h-6 items-center justify-between gap-3 text-sm">
              <span>{questionLabel(group, index)} <HelpTip text={questionTooltip(group)} /></span>
              <span className="text-right tabular-nums">{score?.toFixed(2) ?? (group.comparison_count < 2 ? 'Min. 2 scored attempts required' : 'No score variation')}</span>
            </div>
            {score !== null && <div aria-hidden="true" className="relative mt-1 h-1.5 rounded bg-slate-200 workspace-dark:bg-slate-700">
              <span className="absolute inset-y-0 left-1/2 w-px bg-slate-500" />
              <span className="absolute inset-y-0 rounded bg-indigo-500" style={{
                left: `${50 + Math.min(0, score) * 50}%`,
                width: `${Math.abs(score) * 50}%`,
              }} />
            </div>}
            {score === null && <div aria-hidden="true" className="mt-1 h-1.5" />}
          </li>;
        })}
      </ol>
    </figure>
    <QuestionBarChart groups={groups} title="Average time per question"
      help="Time from opening a question to switching questions, submitting, or reaching the deadline. Includes revisits and idle time."
      value={group => group.average_seconds} format={duration} maximum={maxSeconds} />
    <QuestionBarChart groups={groups} title="Average rubric score"
      help="Average points earned on the question, scaled to 10. For example, 1.5/10 means candidates earned 15% of the available rubric points on average."
      value={group => group.mean_points} format={value => `${Number(value).toFixed(1)}/10`} maximum={10} />
    <QuestionBarChart groups={groups} title="Attempts per question"
      help="Counts submitted interviews in which the candidate opened the question. Revisits within the same interview count once."
      value={attempts} format={String} maximum={Math.max(1, ...groups.map(attempts))} />
  </div>;
}

export function Analytics({ job, token }: { job: string; token: string }) {
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);
  const loading = !stats && !error;

  useEffect(() => {
    const controller = new AbortController();
    void request<Stats>(`/api/v1/jobs/${job}/analytics`, { signal: controller.signal }, token)
      .then(response => {
        if (controller.signal.aborted) return;
        if (response.groups.some(group => !group.question_id || !Number.isFinite(group.comparison_count) ||
            !Number.isFinite(group.timing_count) || group.average_seconds === undefined)) {
          setError('The running backend returned an older analytics response. Restart the backend, then retry.');
          return;
        }
        setStats(response);
      })
      .catch(() => {
        if (!controller.signal.aborted) setError('Unable to load question performance.');
      });
    return () => controller.abort();
  }, [job, token, attempt]);

  const groups = stats?.groups ?? [];

  return <>
    {loading && <p role="status">Loading question performance…</p>}
    {error && <div role="alert" className="error">
      <p>{error}</p>
      <button type="button" className="text-button" onClick={() => { setError(''); setAttempt(value => value + 1); }}>Retry</button>
    </div>}
    {stats && (groups.length ? <QuestionCharts groups={groups} /> : <p>Question performance will appear when a question set is ready.</p>)}
  </>;
}
