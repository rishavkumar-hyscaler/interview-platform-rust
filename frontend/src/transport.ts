import { logEvent, safeRoute } from './observability';
export const apiBaseUrl = (import.meta.env.VITE_API_BASE_URL ?? '').replace(/\/$/, '');

/** Common transport for JSON, media and SSE. Never logs bodies, tokens or query strings. */
export async function loggedFetch(path: string, options: RequestInit = {}): Promise<Response> {
  const headers = new Headers(options.headers);
  const suppliedId = headers.get('X-Request-ID');
  const requestId = suppliedId && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(suppliedId) ? suppliedId : crypto.randomUUID();
  headers.set('X-Request-ID', requestId);
  const fields = { request_id: requestId, method: options.method ?? 'GET', route: safeRoute(path) };
  const started = performance.now();
  logEvent('debug', 'request.started', fields);
  try {
    const response = await fetch(`${apiBaseUrl}${path}`, { ...options, headers });
    logEvent(response.ok ? 'debug' : 'warn', 'request.response', { ...fields, status: response.status, duration_ms: Math.round(performance.now() - started) });
    return response;
  } catch (error) {
    logEvent(options.signal?.aborted ? 'debug' : 'error', 'request.failed', { ...fields, outcome: options.signal?.aborted ? 'cancelled' : 'network_error', duration_ms: Math.round(performance.now() - started) });
    throw error;
  }
}
