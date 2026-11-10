import { useEffect, useState, type ReactNode } from 'react';

/** Hold a browser-managed exclusive lock for the lifetime of this invitation view. */
export function SingleTab({
  token,
  children,
}: {
  token: string;
  children: ReactNode;
}) {
  const [state, setState] = useState<
    'waiting' | 'active' | 'unsupported' | 'failed'
  >('waiting');
  useEffect(() => {
    setState('waiting');
    if (!navigator.locks || !crypto.subtle) {
      setState('unsupported');
      return;
    }
    const controller = new AbortController();
    let release: (() => void) | undefined;
    let disposed = false;
    async function acquire() {
      // Lock names are visible to other scripts on this origin; never put the bearer token there.
      const digest = await crypto.subtle.digest(
        'SHA-256',
        new TextEncoder().encode(token),
      );
      if (disposed) return;
      const name = `interview-active:${Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('')}`;
      await navigator.locks.request(
        name,
        { mode: 'exclusive', signal: controller.signal },
        async () => {
          if (disposed) return;
          await new Promise<void>((resolve) => {
            release = resolve;
            setState('active');
          });
        },
      );
    }
    void acquire().catch(() => {
      if (!disposed) setState('failed');
    });
    return () => {
      disposed = true;
      controller.abort();
      release?.();
    };
  }, [token]);
  if (state === 'active') return children;
  return (
    <main className="access-page">
      <section className="access-card candidate-card">
        <p className="eyebrow">INTERVIEW ACCESS</p>
        <h1>
          {state === 'waiting'
            ? 'Opening your interview'
            : 'Browser support needed'}
        </h1>
        {state === 'waiting' ? (
          <>
            <p role="status">
              If this interview is open in another tab, continue there or close
              that tab. This page will resume automatically when it becomes
              available.
            </p>
            <p className="muted">
              The interview timer continues while you wait. Saved answers stay
              available.
            </p>
          </>
        ) : (
          <p role="alert">
            {state === 'unsupported'
              ? 'Use a current browser on localhost or HTTPS to open the interview in a single active tab.'
              : 'Unable to secure this interview tab. Close other interview tabs and refresh this page.'}
          </p>
        )}
      </section>
    </main>
  );
}
