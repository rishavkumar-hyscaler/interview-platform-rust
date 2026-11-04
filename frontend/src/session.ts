// Per-tab persistence survives refresh without keeping credentials after the tab
// closes. Persist only the opaque token; identity and scope come from /api/me.
import { apiBaseUrl } from './api';
const sessionKey = `interview:recruiter:${apiBaseUrl}`;
export function savedToken(): string | null {
  try {
    const token = sessionStorage.getItem(sessionKey);
    return token && /^[a-f0-9]{64}$/.test(token) ? token : null;
  } catch { return null; }
}
export function rememberToken(token: string): void {
  try { sessionStorage.setItem(sessionKey, token); } catch { /* Storage can be disabled by browser policy. */ }
}
export function forgetToken(): void {
  try { sessionStorage.removeItem(sessionKey); } catch { /* In-memory logout still works. */ }
}
