/** Browser console logging. Route templates only; never log URLs, bodies or error messages. */
export type LogLevel = 'debug' | 'info' | 'warn' | 'error';
const levels = { debug: 0, info: 1, warn: 2, error: 3, silent: 4 };
export function safeRoute(path: string): string {
  const pathname = path.split(/[?#]/, 1)[0];
  const routes: [RegExp, string | ((match: RegExpMatchArray) => string)][] = [
    [
      /^\/api\/v1\/candidate\/events\/[^/]+\/evidence\/[^/]+$/,
      '/api/v1/candidate/events/:event/evidence/:asset',
    ],
    [
      /^\/api\/v1\/sessions\/[^/]+\/evidence\/[^/]+$/,
      '/api/v1/sessions/:session/evidence/:asset',
    ],
    [/^\/health\/(live|ready)$/, (match) => `/health/${match[1]}`],
    [
      /^\/api\/auth\/(register|login|logout)$/,
      (match) => `/api/auth/${match[1]}`,
    ],
    [/^\/api\/me$/, '/api/me'],
    [
      /^\/api\/v1\/(summary|jobs|sessions|audit|metrics)$/,
      (match) => `/api/v1/${match[1]}`,
    ],
    [
      /^\/api\/v1\/jobs\/[^/]+\/(invitations|analytics|question-sets)$/,
      (match) => `/api/v1/jobs/:job/${match[1]}`,
    ],
    [/^\/api\/v1\/sessions\/[^/]+$/, '/api/v1/sessions/:session'],
    [
      /^\/api\/v1\/sessions\/[^/]+\/(invitation|generation|assessment|live|report|integrity|annotations|review|recordings)$/,
      (match) => `/api/v1/sessions/:session/${match[1]}`,
    ],
    [
      /^\/api\/v1\/sessions\/[^/]+\/assessment\/retry$/,
      '/api/v1/sessions/:session/assessment/retry',
    ],
    [
      /^\/api\/v1\/sessions\/[^/]+\/recordings\/[^/]+$/,
      '/api/v1/sessions/:session/recordings/:recording',
    ],
    [
      /^\/api\/v1\/candidate\/(session|room|recordings|events|recommendation)$/,
      (match) => `/api/v1/candidate/${match[1]}`,
    ],
    [
      /^\/api\/v1\/candidate\/session\/(ready|start|submit)$/,
      (match) => `/api/v1/candidate/session/${match[1]}`,
    ],
    [
      /^\/api\/v1\/candidate\/answers\/[^/]+$/,
      '/api/v1/candidate/answers/:question',
    ],
    [
      /^\/api\/v1\/candidate\/recordings\/[^/]+$/,
      '/api/v1/candidate/recordings/:recording',
    ],
    [
      /^\/api\/v1\/candidate\/questions\/[^/]+\/recordings\/[^/]+$/,
      '/api/v1/candidate/questions/:question/recordings/:recording',
    ],
  ];
  for (const [pattern, template] of routes) {
    const match = pathname.match(pattern);
    if (match) return typeof template === 'string' ? template : template(match);
  }
  return 'unmatched';
}

type Fields = {
  request_id?: string;
  route?: string;
  method?: string;
  status?: number;
  duration_ms?: number;
  outcome?: string;
  task_id?: string;
  operation?: string;
};
export function logEvent(level: LogLevel, event: string, fields: Fields = {}) {
  const configured =
    import.meta.env?.VITE_LOG_LEVEL ??
    (import.meta.env?.DEV ? 'debug' : 'warn');
  const threshold =
    configured in levels
      ? levels[configured as keyof typeof levels]
      : levels.warn;
  if (levels[level] < threshold) return;
  console[level]({ timestamp: new Date().toISOString(), event, ...fields });
}
export function startTask(operation: string, requestId?: string) {
  const started = performance.now();
  const fields = {
    operation,
    task_id: crypto.randomUUID(),
    ...(requestId ? { request_id: requestId } : {}),
  };
  logEvent('debug', 'task.started', fields);
  return (outcome: 'succeeded' | 'failed' | 'cancelled') =>
    logEvent(outcome === 'failed' ? 'warn' : 'debug', 'task.completed', {
      ...fields,
      outcome,
      duration_ms: Math.round(performance.now() - started),
    });
}
