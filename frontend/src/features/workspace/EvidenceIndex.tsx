import { useEffect, useState } from 'react';
import { request } from '../../api';
type IndexStatus = { status: string; attempts: number; max_attempts: number; configured: boolean; last_error?: string };
export function EvidenceIndex({ session, token }: { session: string; token: string }) {
  const [state, setState] = useState<IndexStatus | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    const controller = new AbortController(); let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const next = await request<IndexStatus>(`/api/v1/sessions/${session}/rag`, { signal: controller.signal }, token);
        if (controller.signal.aborted) return;
        setState(next); setError('');
      } catch { if (!controller.signal.aborted) setError('Evidence index status unavailable'); }
      // Late uploads and reviewer annotations can invalidate an already-complete index.
      if (!controller.signal.aborted) timer = setTimeout(() => void poll(), 5000);
    }
    void poll(); return () => { controller.abort(); clearTimeout(timer); };
  }, [session, token, refresh]);
  async function retry() {
    setBusy(true); setError('');
    try {
      await request(`/api/v1/sessions/${session}/rag`, { method: 'POST' }, token);
      setRefresh(value => value + 1);
    } catch (failure) { setError(failure instanceof Error ? failure.message : 'Unable to retry evidence indexing'); }
    finally { setBusy(false); }
  }
  const label = state?.status === 'completed' ? 'Evidence search ready · saved vectors'
    : state?.status === 'awaiting_scoring' ? 'Evidence vectors will be created after scoring completes.'
    : state?.status === 'failed' ? `Evidence indexing failed after ${state.attempts} attempts. Saved scores are unaffected.`
    : state?.status === 'running' ? `Preparing evidence search · attempt ${state.attempts}/${state.max_attempts}`
    : 'Evidence indexing queued';
  return <div className="configuration-note"><p role="status">{state ? label : 'Loading evidence index status…'}</p>{state && state.status !== 'completed' && <small>Session keyword search remains available while vectors are pending.</small>}{state && !state.configured && <p>AI configuration is unavailable; automatic indexing is waiting.</p>}{state?.status === 'failed' && <button type="button" className="button secondary" disabled={busy || !state.configured} onClick={() => void retry()}>{busy ? 'Queuing…' : 'Retry evidence indexing'}</button>}{error && <p role="alert">{error}</p>}</div>;
}
