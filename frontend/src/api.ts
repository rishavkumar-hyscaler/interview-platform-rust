export const apiBaseUrl = (import.meta.env.VITE_API_BASE_URL ?? "http://localhost:8080").replace(/\/$/, "");

export class ApiError extends Error {
  constructor(message: string, readonly status: number) { super(message); this.name = 'ApiError'; }
}

export async function request<T>(path: string, options: RequestInit = {}, token?: string): Promise<T> {
  const response = await fetch(`${apiBaseUrl}${path}`, {
    ...options,
    headers: { 'Content-Type': 'application/json', ...(token ? { Authorization: `Bearer ${token}` } : {}), ...options.headers },
  });
  if (!response.ok) {
    const body: unknown = await response.json().catch(() => null);
    if (response.status === 401 && token) window.dispatchEvent(new CustomEvent('interview:unauthorized', { detail: token }));
    throw new ApiError(body && typeof body === 'object' && 'error' in body ? String(body.error) : 'Request failed', response.status);
  }
  return response.status === 204 ? undefined as T : response.json() as Promise<T>;
}
