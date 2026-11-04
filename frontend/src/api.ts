export const apiBaseUrl = (import.meta.env.VITE_API_BASE_URL ?? "").replace(/\/$/, "");

export class ApiError extends Error {
  constructor(message: string, readonly status: number) { super(message); this.name = 'ApiError'; }
}

export async function request<T>(path: string, options: RequestInit = {}, token?: string): Promise<T> {
  let response: Response;
  const controller = new AbortController();
  const cancel = () => controller.abort();
  if (options.signal?.aborted) controller.abort();
  options.signal?.addEventListener('abort', cancel, { once: true });
  const timeout = setTimeout(cancel, 15000);
  try {
    response = await fetch(`${apiBaseUrl}${path}`, {
      ...options,
      signal: controller.signal,
      headers: { 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}), ...options.headers },
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
  return response.status === 204 ? undefined as T : response.json() as Promise<T>;
}
