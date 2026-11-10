import { Button } from '../../components/ui/button';
import { Select } from '../../components/ui/select';
import { DashboardCard } from './DashboardCard';
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

type Stats = {
  groups: ScoreGroup[];
  sets?: { set_number: number; status: string }[];
};

const setStatus: Record<string, string> = {
  completed: 'Ready',
  pending: 'Queued',
  running: 'Preparing',
  failed: 'Preparation failed',
};

function readableLabel(value: string) {
  return value.replace(/[_-]+/g, ' ').replace(/\b\w+/g, (word) => {
    const acronyms: Record<string, string> = {
      api: 'API',
      apis: 'APIs',
      db: 'DB',
      jwt: 'JWT',
      rbac: 'RBAC',
      html: 'HTML',
      css: 'CSS',
    };
    return (
      acronyms[word.toLowerCase()] ??
      word.charAt(0).toUpperCase() + word.slice(1)
    );
  });
}

const questionTypes: Record<string, string> = {
  text: 'Written response',
  code: 'Coding exercise',
  scenario: 'Scenario',
};

const questionPerformanceHelp = `Pearson correlation compares scores on this question with scores on the rest of the interview. Positive values mean higher scorers on this question tended to score higher on the rest, and lower scorers tended to score lower. Near zero means little linear relationship. Negative values show the reverse pattern.\n\nThe chart can show a value from two comparable interviews if both sets of scores vary.`;

function separation(group: ScoreGroup) {
  if (group.comparison_count === 0) return 'No paired scores yet';
  if (group.comparison_count === 1)
    return 'Correlation requires at least two comparable interviews';
  if (group.item_total_correlation == null)
    return 'Scores did not vary enough for correlation';
  if (group.item_total_correlation >= 0.3)
    return 'Separates stronger and weaker scores';
  if (group.item_total_correlation <= -0.2)
    return 'Reverse pattern · review question';
  if (Math.abs(group.item_total_correlation) < 0.2)
    return 'Little separation · review question';
  return 'Some separation';
}

function duration(seconds: number) {
  const rounded = Math.round(seconds);
  return `${Math.floor(rounded / 60)}m ${rounded % 60}s`;
}

function questionTooltip(group: ScoreGroup) {
  const average =
    group.mean_points == null
      ? 'Awaiting scores'
      : `${Number(group.mean_points).toFixed(1)}/10 average rubric score`;
  return [
    group.prompt,
    separation(group),
    `${readableLabel(group.competency_id)} · ${questionTypes[group.kind] ?? readableLabel(group.kind)} · ${readableLabel(group.difficulty)}`,
    `${average} · ${group.sample_size} scored responses`,
    `${group.attempt_count ?? group.timing_count} attempts · ${group.comparison_count} comparable interviews`,
  ].join('\n\n');
}

function questionLabel(group: ScoreGroup, index: number) {
  return group.set_number != null && group.position != null
    ? `Set ${group.set_number} · Q${group.position}`
    : `Q${index + 1}`;
}

function QuestionBarChart({
  index: cardIndex,
  groups,
  title,
  help,
  value,
  format,
  maximum,
}: {
  index: number;
  groups: ScoreGroup[];
  title: string;
  help: string;
  value: (group: ScoreGroup) => number | null;
  format: (value: number) => string;
  maximum: number;
}) {
  return (
    <DashboardCard variant="chart" index={cardIndex}>
      <figcaption className="mb-3 flex shrink-0 items-center justify-between gap-2 text-control font-medium">
        {title}
        <HelpTip text={help} />
      </figcaption>
      <ol
        aria-label={title}
        tabIndex={0}
        className="m-0 grid min-h-0 flex-1 content-start list-none gap-3 overflow-y-auto overscroll-contain p-0 pr-2 [scrollbar-gutter:stable]"
      >
        {groups.map((group, index) => {
          const amount = value(group);
          return (
            <li key={group.question_id}>
              <div className="flex min-h-6 items-center justify-between gap-3 text-control">
                <span>
                  {questionLabel(group, index)}{' '}
                  <HelpTip text={questionTooltip(group)} />
                </span>
                <span className="text-right tabular-nums">
                  {amount === null ? 'Unavailable' : format(amount)}
                </span>
              </div>
              <div
                aria-hidden="true"
                className="mt-1 h-1.5 overflow-hidden rounded bg-slate-200 workspace-dark:bg-slate-700"
              >
                {amount !== null && (
                  <div
                    className="h-full origin-left rounded bg-indigo-500 motion-safe:animate-chart-bar"
                    style={{ width: `${(100 * amount) / maximum}%` }}
                  />
                )}
              </div>
            </li>
          );
        })}
      </ol>
    </DashboardCard>
  );
}

const attempts = (group: ScoreGroup) =>
  group.attempt_count ?? group.timing_count;

function AttemptsChart({ groups }: { groups: ScoreGroup[] }) {
  const maximum = Math.max(1, ...groups.map(attempts));

  return (
    <DashboardCard variant="chart" index={3}>
      <figcaption className="mb-3 flex shrink-0 items-center justify-between gap-2 text-control font-medium">
        Attempts per question
        <HelpTip text="Counts submitted interviews in which the candidate opened the question. Revisits within the same interview count once." />
      </figcaption>
      <ol
        aria-label="Attempts per question"
        tabIndex={0}
        className="m-0 flex min-h-0 flex-1 list-none overflow-auto overscroll-contain p-0 pb-2"
      >
        {groups.map((group, index) => {
          const count = attempts(group);
          return (
            <li
              key={group.question_id}
              className="min-w-28 flex-1 text-center text-control"
            >
              <div className="flex h-40 flex-col items-center justify-end border-b border-slate-300 workspace-dark:border-slate-600">
                <span className="mb-1 tabular-nums">{count}</span>
                <div
                  aria-hidden="true"
                  className="w-9 origin-bottom rounded-t-md bg-indigo-500 motion-safe:animate-chart-column"
                  style={{ height: `${(75 * count) / maximum}%` }}
                />
              </div>
              <div className="mt-2 whitespace-nowrap">
                {questionLabel(group, index)}{' '}
                <HelpTip text={questionTooltip(group)} />
              </div>
            </li>
          );
        })}
      </ol>
    </DashboardCard>
  );
}

function QuestionCharts({ groups }: { groups: ScoreGroup[] }) {
  const maxSeconds = Math.max(
    1,
    ...groups.map((group) => Number(group.average_seconds ?? 0)),
  );

  return (
    <div className="my-4 grid grid-cols-1 gap-4 lg:grid-cols-2">
      <DashboardCard variant="chart" index={0}>
        <figcaption className="mb-3 flex shrink-0 items-center justify-between gap-2 text-control font-medium">
          <span>
            Score separation{' '}
            <span className="ml-2 text-detail font-normal text-slate-600 workspace-dark:text-slate-400">
              −1 to +1
            </span>
          </span>
          <HelpTip text={questionPerformanceHelp} />
        </figcaption>
        <ol
          aria-label="Score separation"
          tabIndex={0}
          className="m-0 flex min-h-0 flex-1 list-none overflow-auto overscroll-contain p-0 pb-2"
        >
          {groups.map((group, index) => {
            const score =
              group.item_total_correlation == null
                ? null
                : Number(group.item_total_correlation);
            return (
              <li
                key={group.question_id}
                className="min-w-28 flex-1 text-center text-control"
              >
                <div className="mb-2 h-5 px-1 text-detail tabular-nums">
                  {score === null
                    ? '—'
                    : `${score > 0 ? '+' : ''}${score.toFixed(2)}`}
                </div>
                <div className="relative h-24 border-y border-dashed border-slate-200 workspace-dark:border-slate-700">
                  <div
                    aria-hidden="true"
                    className="absolute inset-x-0 top-1/2 border-t border-slate-300 workspace-dark:border-slate-600"
                  />
                  {score !== null && (
                    <div
                      aria-hidden="true"
                      className="absolute left-1/2 w-9 -translate-x-1/2 rounded bg-indigo-500 motion-safe:animate-chart-column"
                      style={{
                        bottom: score >= 0 ? '50%' : undefined,
                        top: score < 0 ? '50%' : undefined,
                        height: `${Math.abs(score) * 50}%`,
                        transformOrigin: score < 0 ? 'top' : 'bottom',
                      }}
                    />
                  )}
                </div>
                <div className="mt-3 whitespace-nowrap">
                  {questionLabel(group, index)}{' '}
                  <HelpTip text={questionTooltip(group)} />
                </div>
                {score === null && (
                  <div className="mt-1 px-1 text-detail text-slate-600 workspace-dark:text-slate-400">
                    {group.comparison_count < 2
                      ? 'Needs 2 comparable interviews'
                      : 'No score variation'}
                  </div>
                )}
              </li>
            );
          })}
        </ol>
      </DashboardCard>
      <QuestionBarChart
        index={1}
        groups={groups}
        title="Average time per question"
        help="Time from opening a question to switching questions, submitting, or reaching the deadline. Includes revisits and idle time."
        value={(group) => group.average_seconds}
        format={duration}
        maximum={maxSeconds}
      />
      <QuestionBarChart
        index={2}
        groups={groups}
        title="Average rubric score"
        help="Average points earned on the question, scaled to 10. For example, 1.5/10 means candidates earned 15% of the available rubric points on average."
        value={(group) => group.mean_points}
        format={(value) => `${Number(value).toFixed(1)}/10`}
        maximum={10}
      />
      <AttemptsChart groups={groups} />
    </div>
  );
}

function FilteredQuestionCharts({ groups }: { groups: ScoreGroup[] }) {
  const [set, setSet] = useState('');
  const [question, setQuestion] = useState('');
  const [sort, setSort] = useState('question');
  const sets = [
    ...new Set(
      groups.flatMap((group) =>
        group.set_number == null ? [] : [group.set_number],
      ),
    ),
  ].sort((left, right) => left - right);
  const available = groups.filter(
    (group) => !set || String(group.set_number) === set,
  );
  const visible = available.filter(
    (group) => !question || group.question_id === question,
  );

  if (sort === 'attempts')
    visible.sort((left, right) => attempts(right) - attempts(left));
  if (sort === 'time')
    visible.sort(
      (left, right) =>
        (right.average_seconds ?? -1) - (left.average_seconds ?? -1),
    );
  if (sort === 'score')
    visible.sort(
      (left, right) => (right.mean_points ?? -1) - (left.mean_points ?? -1),
    );

  return (
    <>
      <div className="mt-4 flex flex-wrap justify-end gap-3">
        <label className="flex items-center gap-2 text-control">
          Set
          <Select
            value={set}
            onChange={(event) => {
              setSet(event.target.value);
              setQuestion('');
            }}
          >
            <option value="">All sets</option>
            {sets.map((number) => (
              <option key={number} value={number}>
                Set {number}
              </option>
            ))}
          </Select>
        </label>
        <label className="flex items-center gap-2 text-control">
          Question
          <Select
            value={question}
            onChange={(event) => setQuestion(event.target.value)}
          >
            <option value="">All questions</option>
            {available.map((group, index) => (
              <option key={group.question_id} value={group.question_id}>
                {questionLabel(group, index)}
              </option>
            ))}
          </Select>
        </label>
        <label className="flex items-center gap-2 whitespace-nowrap text-control">
          Sort by
          <Select
            value={sort}
            onChange={(event) => setSort(event.target.value)}
          >
            <option value="question">Set and question</option>
            <option value="attempts">Most attempts</option>
            <option value="time">Longest average time</option>
            <option value="score">Highest average score</option>
          </Select>
        </label>
      </div>
      <QuestionCharts groups={visible} />
    </>
  );
}

export function Analytics({ job, token }: { job: string; token: string }) {
  const [stats, setStats] = useState<Stats | null>(null);
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);
  const loading = !stats && !error;

  useEffect(() => {
    const controller = new AbortController();
    void request<Stats>(
      `/api/v1/jobs/${job}/analytics`,
      { signal: controller.signal },
      token,
    )
      .then((response) => {
        if (controller.signal.aborted) return;
        if (
          response.groups.some(
            (group) =>
              !group.question_id ||
              !Number.isFinite(group.comparison_count) ||
              !Number.isFinite(group.timing_count) ||
              group.average_seconds === undefined,
          )
        ) {
          setError(
            'The running backend returned an older analytics response. Restart the backend, then retry.',
          );
          return;
        }
        setStats(response);
      })
      .catch(() => {
        if (!controller.signal.aborted)
          setError('Unable to load question performance.');
      });
    return () => controller.abort();
  }, [job, token, attempt]);

  const groups = stats?.groups ?? [];

  return (
    <>
      {loading && <p role="status">Loading question performance…</p>}
      {error && (
        <div role="alert" className="error">
          <p>{error}</p>
          <Button
            type="button"
            variant="text"
            onClick={() => {
              setError('');
              setAttempt((value) => value + 1);
            }}
          >
            Retry
          </Button>
        </div>
      )}
      {!!stats?.sets?.length && (
        <ul
          aria-label="Question set status"
          className="my-3 flex list-none flex-wrap gap-2 p-0"
        >
          {stats.sets.map((set) => (
            <li key={set.set_number} className="pill">
              Set {set.set_number} ·{' '}
              {setStatus[set.status] ?? readableLabel(set.status)}
            </li>
          ))}
        </ul>
      )}
      {stats &&
        (groups.length ? (
          <FilteredQuestionCharts groups={groups} />
        ) : (
          <p>Question performance will appear when a question set is ready.</p>
        ))}
    </>
  );
}
