import { useCallback, useEffect, useRef, useState } from 'react';
import { candidateApi } from '../api';
export type BrowserObservation = {
  id: string;
  kind: string;
  client_time: number;
  count?: number;
};
type Listener = (event: BrowserObservation) => void;
export function useObservations(
  token: string,
  session: string | undefined,
  active: boolean,
) {
  const queue = useRef<BrowserObservation[]>([]);
  const listeners = useRef(new Set<Listener>());
  const send = useRef<() => void>(() => {});
  const [status, setStatus] = useState('Browser monitoring ready');
  const current = useRef(session);
  current.current = session;
  const persist = useCallback(() => {
    if (!current.current) return;
    try {
      localStorage.setItem(
        `interview-events:${current.current}`,
        JSON.stringify(queue.current),
      );
    } catch {
      setStatus('Event recovery unavailable; keep this tab open');
    }
  }, []);
  useEffect(() => {
    if (!session || !active) return;
    let disposed = false;
    let sending = false;
    try {
      const stored = JSON.parse(
        localStorage.getItem(`interview-events:${session}`) ?? '[]',
      );
      queue.current = Array.isArray(stored) ? stored.slice(0, 1000) : [];
    } catch {
      queue.current = [];
    }
    async function flush() {
      if (disposed || sending || !queue.current.length) return;
      sending = true;
      const batch = queue.current.slice(0, 40);
      try {
        await candidateApi(token).events(batch);
        if (disposed) return;
        queue.current = queue.current.filter(
          (event) => !batch.some((sent) => sent.id === event.id),
        );
        persist();
        setStatus(
          queue.current.length
            ? 'Syncing observations…'
            : 'Browser observations synced',
        );
      } catch {
        if (!disposed)
          setStatus('Browser observations pending · retrying connection');
      } finally {
        sending = false;
      }
    }
    send.current = () => {
      void flush();
    };
    const timer = setInterval(send.current, 5000);
    const online = send.current;
    window.addEventListener('online', online);
    send.current();
    return () => {
      disposed = true;
      send.current = () => {};
      clearInterval(timer);
      window.removeEventListener('online', online);
    };
  }, [token, session, active, persist]);
  const record = useCallback(
    (kind: string, count?: number) => {
      if (!current.current) return null;
      if (queue.current.length >= 1000) {
        setStatus('Event buffer full · reconnect to sync');
        return null;
      }
      const event = {
        id: crypto.randomUUID(),
        kind,
        client_time: Date.now(),
        ...(count === undefined
          ? {}
          : { count: Math.max(0, Math.min(count, 1000000)) }),
      };
      queue.current.push(event);
      persist();
      send.current();
      listeners.current.forEach((listener) => listener(event));
      return event;
    },
    [persist],
  );
  const subscribe = useCallback((listener: Listener) => {
    listeners.current.add(listener);
    return () => {
      listeners.current.delete(listener);
    };
  }, []);
  return { record, subscribe, status };
}
export type Observations = ReturnType<typeof useObservations>;
