import { QueryClientProvider } from '@tanstack/react-query';
import { queryClient } from './queryClient';
import { Button } from './components/ui/button';
import { StrictMode, useEffect, useState, type FormEvent } from 'react';
import { createRoot } from 'react-dom/client';
import { ApiError, request } from './api';
import { savedToken, rememberToken, forgetToken } from './session';
import { CandidateExperience } from './features/interview/CandidateExperience';
import { Workspace } from './features/workspace/Workspace';
import './style.css';

type Session = { token: string; user_id: string; role: 'interviewer' };
// Fragments never reach the HTTP server. Keep the invitation in the fragment
// so refresh/back/forward can restore candidate access without recruiter login.
function invitationFromLocation(): string | null {
  return new URLSearchParams(window.location.hash.slice(1)).get('invite');
}
function App() {
  const [invitationToken, setInvitationToken] = useState(
    invitationFromLocation,
  );
  useEffect(() => {
    const updateInvitation = () => setInvitationToken(invitationFromLocation());
    window.addEventListener('hashchange', updateInvitation);
    window.addEventListener('popstate', updateInvitation);
    return () => {
      window.removeEventListener('hashchange', updateInvitation);
      window.removeEventListener('popstate', updateInvitation);
    };
  }, []);
  const [register, setRegister] = useState(false);
  const [session, setSession] = useState<Session | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [restoring, setRestoring] = useState(
    invitationToken === null && !!savedToken(),
  );
  const [restoreFailed, setRestoreFailed] = useState(false);
  const [restoreAttempt, setRestoreAttempt] = useState(0);
  useEffect(() => {
    const token = savedToken();
    if (!token || invitationToken !== null) {
      setRestoring(false);
      return;
    }
    let active = true;
    setRestoring(true);
    setRestoreFailed(false);
    request<Omit<Session, 'token'>>(
      '/api/me',
      { signal: AbortSignal.timeout(10000) },
      token,
    )
      .then((profile) => {
        if (active) {
          setSession({ ...profile, token });
          setError('');
        }
      })
      .catch((error) => {
        if (!active) return;
        if (error instanceof ApiError && error.status === 401) {
          queryClient.clear();
          forgetToken();
          setSession(null);
          setError('Your session expired. Please sign in again.');
        } else {
          setRestoreFailed(true);
          setError(
            'Unable to verify your session. Check the connection and retry.',
          );
        }
      })
      .finally(() => {
        if (active) setRestoring(false);
      });
    return () => {
      active = false;
    };
  }, [restoreAttempt, invitationToken]);
  useEffect(() => {
    if (!restoreFailed || invitationToken !== null) return;
    function retry() {
      setRestoreAttempt((value) => value + 1);
    }
    window.addEventListener('online', retry);
    return () => window.removeEventListener('online', retry);
  }, [restoreFailed, invitationToken]);
  useEffect(() => {
    function unauthorized(event: Event) {
      if (session && (event as CustomEvent<string>).detail === session.token) {
        queryClient.clear();
        forgetToken();
        setSession(null);
        setError('Your session expired. Please sign in again.');
      }
    }
    window.addEventListener('interview:unauthorized', unauthorized);
    return () =>
      window.removeEventListener('interview:unauthorized', unauthorized);
  }, [session]);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError('');
    const data = new FormData(event.currentTarget);
    try {
      const next = await request<Session>(
        `/api/auth/${register ? 'register' : 'login'}`,
        { method: 'POST', body: JSON.stringify(Object.fromEntries(data)) },
      );
      rememberToken(next.token);
      setSession(next);
      setRestoreFailed(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Unable to sign in');
    } finally {
      setBusy(false);
    }
  }
  async function logout() {
    if (!session) return;
    setBusy(true);
    setError('');
    try {
      await request<void>(
        '/api/auth/logout',
        { method: 'POST' },
        session.token,
      );
      queryClient.clear();
      forgetToken();
      setSession(null);
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        queryClient.clear();
        forgetToken();
        setSession(null);
      } else {
        setError('Sign out failed. Please retry.');
      }
    } finally {
      setBusy(false);
    }
  }
  if (invitationToken !== null)
    return (
      <CandidateExperience key={invitationToken} token={invitationToken} />
    );
  if (restoring || restoreFailed)
    return (
      <main className="access-page grid-cols-1! place-items-center!">
        <section
          className="access-card w-full max-w-md"
          aria-labelledby="reconnect-title"
        >
          <h2 id="reconnect-title">Reconnect to your session</h2>
          {restoring ? (
            <p role="status">Checking your connection…</p>
          ) : (
            <>
              <p role="alert" className="error">
                {error}
              </p>
              <div className="grid gap-2">
                <Button
                  variant="standard"
                  onClick={() => setRestoreAttempt((value) => value + 1)}
                >
                  Retry connection
                </Button>
                <Button
                  variant="text"
                  className="w-full"
                  onClick={() => {
                    queryClient.clear();
                    forgetToken();
                    setSession(null);
                    setRestoreFailed(false);
                    setError('');
                  }}
                >
                  Sign in with another account
                </Button>
              </div>
            </>
          )}
        </section>
      </main>
    );
  if (session)
    return (
      <>
        <Workspace token={session.token} onLogout={() => void logout()} />
        {error && (
          <p className="toast error" role="alert">
            {error}
          </p>
        )}
      </>
    );
  return (
    <main className="access-page">
      <div className="access-intro">
        <a className="logo" href="#">
          <span className="logo-mark">i</span> interview /
        </a>
        <p className="eyebrow">BETTER SIGNAL. BETTER HIRING.</p>
        <h1>
          Great interviews.
          <br />
          <em>Grounded in evidence.</em>
        </h1>
        <p>
          Bring structure to your hiring process, from the first job description
          to the final review.
        </p>
        <div className="access-features">
          <span>
            01 <strong>Define the role</strong>
          </span>
          <span>
            02 <strong>Invite your candidates</strong>
          </span>
          <span>
            03 <strong>Review with context</strong>
          </span>
        </div>
      </div>
      <section className="access-card" aria-label="Interviewer access">
        <p className="eyebrow">INTERVIEWER</p>
        <h2>{register ? 'Create an interviewer account' : 'Welcome back'}</h2>
        <p className="muted">
          Sign in to set up interviews and review candidate evidence.
        </p>
        {error && (
          <p role="alert" className="error">
            {error}
          </p>
        )}
        <form onSubmit={submit}>
          <label>
            Email
            <input
              name="email"
              type="email"
              required
              maxLength={254}
              autoComplete="username"
              placeholder="you@company.com"
            />
          </label>
          <label>
            Password
            <input
              name="password"
              type="password"
              required
              minLength={12}
              maxLength={128}
              autoComplete={register ? 'new-password' : 'current-password'}
            />
          </label>
          <Button type="submit" variant="standard" disabled={busy}>
            {busy
              ? 'Please wait…'
              : register
                ? 'Create account →'
                : 'Sign in →'}
          </Button>
          <Button
            type="button"
            variant="text"
            disabled={busy}
            onClick={() => {
              setRegister(!register);
              setError('');
            }}
          >
            {register
              ? 'Already have an account? Sign in'
              : 'New here? Create an interviewer account'}
          </Button>
        </form>
      </section>
    </main>
  );
}
createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
);
