import { useEffect, useState } from 'react';
import { candidateApi } from './api';
type Observation = { id: string; kind: string; client_time: number; count?: number };
export function Proctoring({ token, sessionId }: { token: string; sessionId: string }) {
  const [fullscreen, setFullscreen] = useState(!!document.fullscreenElement);
  const [status, setStatus] = useState('Browser monitoring active');
  const [error, setError] = useState('');
  useEffect(() => {
    const api = candidateApi(token);
    const key = `interview-events:${sessionId}`;
    let queue: Observation[] = [];
    try { const stored: unknown = JSON.parse(localStorage.getItem(key) ?? '[]'); if (Array.isArray(stored)) queue = stored.slice(0, 1000); } catch { /* Continue with in-memory delivery. */ }
    let sending = false; let active = true;
    const persist = () => { try { localStorage.setItem(key, JSON.stringify(queue)); } catch { if (active) setStatus('Event recovery unavailable; keep this tab open'); } };
    async function send() {
      if (sending || !queue.length) return;
      sending = true;
      const batch = queue.slice(0, 40);
      try {
        await api.events(batch);
        queue = queue.filter(event => !batch.some(sent => sent.id === event.id)); persist();
        if (active) setStatus(queue.length ? 'Syncing browser events…' : 'Browser monitoring active');
      } catch { if (active) setStatus('Browser events pending · retrying connection'); }
      finally { sending = false; }
    }
    function record(kind: string, count?: number) {
      if (queue.length >= 1000) { setStatus('Event buffer full · reconnect to sync'); return; }
      queue.push({ id: crypto.randomUUID(), kind, client_time: Date.now(), ...(count === undefined ? {} : { count: Math.min(count, 1000000) }) }); persist(); void send();
    }
    const blur = () => record('focus_lost'); const focus = () => record('focus_returned');
    const visibility = () => record(document.hidden ? 'hidden' : 'visible');
    const full = () => { setFullscreen(!!document.fullscreenElement); record(document.fullscreenElement ? 'fullscreen_entered' : 'fullscreen_exited'); };
    const copy = () => record('copy');
    const paste = (event: ClipboardEvent) => record('paste', event.clipboardData?.getData('text').length ?? 0);
    const context = () => record('context_menu');
    const device = (event: Event) => { const kind = (event as CustomEvent<string>).detail; record(kind); };
    window.addEventListener('blur', blur); window.addEventListener('focus', focus);
    document.addEventListener('visibilitychange', visibility); document.addEventListener('fullscreenchange', full);
    document.addEventListener('copy', copy); document.addEventListener('paste', paste); document.addEventListener('contextmenu', context);
    window.addEventListener('interview-device', device); window.addEventListener('online', send);
    const timer = setInterval(() => void send(), 5000); void send();
    return () => {
      active = false; clearInterval(timer); window.removeEventListener('blur', blur); window.removeEventListener('focus', focus);
      document.removeEventListener('visibilitychange', visibility); document.removeEventListener('fullscreenchange', full);
      document.removeEventListener('copy', copy); document.removeEventListener('paste', paste); document.removeEventListener('contextmenu', context);
      window.removeEventListener('interview-device', device); window.removeEventListener('online', send);
    };
  }, [token, sessionId]);
  return <div className="proctor-status"><small>{status}. Activity signals are observations, not cheating verdicts.</small>{!fullscreen && <button className="button secondary" onClick={() => { setError(''); void document.documentElement.requestFullscreen().catch(() => setError('Fullscreen unavailable. Browser monitoring continues.')); }}>Enter fullscreen</button>}{error && <small role="alert">{error}</small>}</div>;
}
