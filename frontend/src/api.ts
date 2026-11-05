import { loggedFetch } from './transport';
import { safeRoute, startTask } from './observability';
export { apiBaseUrl } from './transport';

export class ApiError extends Error {
  constructor(message: string, readonly status: number) { super(message); this.name = 'ApiError'; }
}

export async function request<T>(path: string, options: RequestInit = {}, token?: string, timeoutMs = 15000): Promise<T> {
  const requestId = crypto.randomUUID();
  const finishTask = startTask(safeRoute(path), requestId);
  try {
    let response: Response;
    const controller = new AbortController();
    const cancel = () => controller.abort();
    if (options.signal?.aborted) controller.abort();
    options.signal?.addEventListener('abort', cancel, { once: true });
    const timeout = setTimeout(cancel, timeoutMs);
    const headers = new Headers(options.headers);
    if (!headers.has('Content-Type')) headers.set('Content-Type', 'application/json');
    if (token) headers.set('Authorization', `Bearer ${token}`);
    headers.set('X-Request-ID', requestId);
    try {
      response = await loggedFetch(path, {
        ...options,
        signal: controller.signal,
        headers,
      });
    } catch (error) {
      if (options.signal?.aborted) throw error;
      throw new ApiError('Cannot reach the interview server. Make sure the backend is running, then retry.', 0);
    } finally {
      clearTimeout(timeout); options.signal?.removeEventListener('abort', cancel);
    }
    if (!response.ok) {
      const body: unknown = await response.json().catch(() => null);
      if (response.status === 401 && token) window.dispatchEvent(new CustomEvent('interview:unauthorized', { detail: token }));
      throw new ApiError(body && typeof body === 'object' && 'error' in body ? String(body.error) : response.status >= 500 ? 'The interview server is unavailable. Make sure the backend is running, then retry.' : 'Request failed', response.status);
    }
    const value = response.status === 204 ? undefined as T : await response.json() as T;
    finishTask('succeeded');
    return value;
  } catch (error) {
    finishTask(options.signal?.aborted ? 'cancelled' : 'failed');
    throw error;
  }
}
