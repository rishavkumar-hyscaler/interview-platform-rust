import { QueryClientProvider } from '@tanstack/react-query';
import { queryClient } from './queryClient';
import { Button } from './components/ui/button';
import { Card } from './components/ui/card';
import { Input } from './components/ui/input';
import { ArrowRight, Eye, EyeOff, LoaderCircle, LockKeyhole } from 'lucide-react';
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
  const [showPassword, setShowPassword] = useState(false);
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
      <Card
        className="relative w-full max-w-[440px] gap-0 overflow-hidden rounded-3xl border-indigo-100 bg-white px-6 py-8 shadow-xl shadow-indigo-950/5 sm:px-9 sm:py-10"
        aria-label="Interviewer access"
      >
        <div aria-hidden="true" className="absolute inset-x-0 top-0 h-1 bg-gradient-to-r from-indigo-300 via-indigo-500 to-violet-300" />
        <div className="mb-6 flex items-center gap-3">
          <span className="grid size-11 place-items-center rounded-2xl bg-indigo-50 text-indigo-600 ring-1 ring-indigo-100">
            <LockKeyhole className="size-5" aria-hidden="true" />
          </span>
          <span className="text-detail font-medium tracking-[0.16em] text-indigo-600">INTERVIEWER</span>
        </div>
        <h2 className="m-0! text-section-title font-normal tracking-tight text-slate-900">
          {register ? 'Create your account' : 'Welcome back'}
        </h2>
        <p className="mt-2 mb-7 text-control leading-relaxed text-slate-500">
          {register ? 'Your next great interview starts here.' : 'Sign in to your interview workspace.'}
        </p>
        {error && (
          <p role="alert" className="error">
            {error}
          </p>
        )}
        <form onSubmit={submit} className="gap-5">
          <label className="gap-2 text-control font-normal text-slate-600">
            Email address
            <Input
              className="h-12! rounded-xl! bg-slate-50! px-4!"
              name="email"
              type="email"
              required
              maxLength={254}
              autoComplete="username"
              placeholder="you@company.com"
            />
          </label>
          <label className="gap-2 text-control font-normal text-slate-600">
            Password
            <div className="relative">
              <Input
                className="h-12! rounded-xl! bg-slate-50! pr-12! pl-4!"
                name="password"
                type={showPassword ? 'text' : 'password'}
                required
                minLength={12}
                maxLength={128}
                autoComplete={register ? 'new-password' : 'current-password'}
                placeholder={register ? 'At least 12 characters' : 'Enter your password'}
              />
              <Button
                variant="text"
                className="absolute inset-y-1 right-1 grid w-10 place-items-center rounded-lg! border-0! bg-transparent! p-0! text-slate-400! shadow-none! hover:text-indigo-600!"
                aria-label={showPassword ? 'Hide password' : 'Show password'}
                aria-pressed={showPassword}
                onClick={() => setShowPassword(!showPassword)}
              >
                {showPassword ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
              </Button>
            </div>
          </label>
          <Button
            type="submit"
            variant="standard"
            className="mt-1 h-12! w-full gap-2 rounded-xl! border-indigo-600! bg-indigo-600! text-white! shadow-md shadow-indigo-500/15 hover:bg-indigo-700!"
            disabled={busy}
          >
            {busy ? (
              <><LoaderCircle className="size-4 motion-safe:animate-spin" /> Please wait…</>
            ) : (
              <>{register ? 'Create account' : 'Sign in'} <ArrowRight className="size-4" /></>
            )}
          </Button>
        </form>
        <div className="mt-7 flex flex-wrap items-center justify-center gap-x-1.5 gap-y-1 border-t border-slate-100 pt-5 text-control text-slate-500">
          <span>{register ? 'Already registered?' : 'New here?'}</span>
          <Button
            variant="text"
            className="min-h-0! rounded-md! border-0! bg-transparent! px-1! py-1! font-medium! text-indigo-600! shadow-none! hover:text-indigo-800!"
            disabled={busy}
            onClick={() => {
              setRegister(!register);
              setShowPassword(false);
              setError('');
            }}
          >
            {register ? 'Sign in' : 'Create an account'}
          </Button>
        </div>
      </Card>
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
