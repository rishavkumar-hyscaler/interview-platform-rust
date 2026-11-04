import { StrictMode, useEffect, useState, type FormEvent } from 'react';
import { createRoot } from 'react-dom/client';
import { ApiError, request } from './api';
import { savedToken, rememberToken, forgetToken } from './session';
import { Workspace } from './features/workspace/Workspace';
import './style.css';

type Session = { token: string; user_id: string; organization_id: string; role: 'recruiter' };
type CandidateSession = { id: string; job_title: string; state: string; duration_minutes: number; question_count: number };
// Fragments never reach the HTTP server. Keep the invitation in the fragment
// so refresh/back/forward can restore candidate access without recruiter login.
function invitationFromLocation(): string | null {
  return new URLSearchParams(window.location.hash.slice(1)).get('invite');
}
function CandidateAccess({ token }: { token: string }) {
  const [session, setSession] = useState<CandidateSession | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(true);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    let active = true;
    setBusy(true); setError(''); setSession(null);
    request<CandidateSession>('/api/v1/candidate/session', { signal: controller.signal }, token)
      .then(result => { if (active) setSession(result); })
      .catch(error => {
        if (!active) return;
        setError(error instanceof ApiError && error.status === 401
          ? 'This invitation link is invalid or has expired. Ask your recruiter for a current link.'
          : 'Unable to open your invitation. Check your connection and try again.');
      }).finally(() => { if (active) setBusy(false); });
    return () => { active = false; controller.abort(); };
  }, [token, attempt]);
  async function ready() {
    setBusy(true); setError('');
    try { setSession(await request<CandidateSession>('/api/v1/candidate/session/ready', { method: 'POST' }, token)); }
    catch (error) { setError(error instanceof Error ? error.message : 'Unable to accept invitation'); }
    finally { setBusy(false); }
  }
  return <main className="access-page"><div className="access-card"><div className="eyebrow">CANDIDATE INVITATION</div><h1>{session?.job_title ?? 'Your next opportunity.'}</h1>
    {busy && !session && <p role="status">Opening your invitation…</p>}
    {error && <p role="alert" className="error">{error}</p>}
    {session ? <><p>{session.duration_minutes} minute interview · {session.state.replaceAll('_', ' ')}</p>
      {session.state === 'expired' ? <p>This invitation has expired. Please ask your recruiter for a new invitation.</p> : <p className="muted">The interview room and question generation will be available in the next phase. Your current session status is shown above.</p>}
      {session.state === 'invited' && <button className="button" onClick={ready} disabled={busy}>{busy ? 'Accepting…' : 'Accept invitation'}</button>}
      {session.state === 'ready' && <p className="success" role="status">Invitation accepted. Your session is ready for preparation.</p>}
    </> : !busy && <button className="button" onClick={() => setAttempt(value => value + 1)}>Retry invitation</button>}
  </div></main>;
}
function App() {
  const [invitationToken, setInvitationToken] = useState(invitationFromLocation);
  useEffect(() => {
    const updateInvitation = () => setInvitationToken(invitationFromLocation());
    window.addEventListener('hashchange', updateInvitation);
    window.addEventListener('popstate', updateInvitation);
    return () => { window.removeEventListener('hashchange', updateInvitation); window.removeEventListener('popstate', updateInvitation); };
  }, []);
  const [register, setRegister] = useState(false);
  const [session, setSession] = useState<Session | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [restoring, setRestoring] = useState(invitationToken === null && !!savedToken());
  const [restoreFailed, setRestoreFailed] = useState(false);
  const [restoreAttempt, setRestoreAttempt] = useState(0);
  useEffect(() => {
    const token = savedToken();
    if (!token || invitationToken !== null) { setRestoring(false); return; }
    let active = true;
    setRestoring(true); setRestoreFailed(false);
    request<Omit<Session, 'token'>>('/api/me', {}, token).then(profile => {
      if (active) { setSession({ ...profile, token }); setError(''); }
    }).catch(error => {
      if (!active) return;
      if (error instanceof ApiError && error.status === 401) {
        forgetToken(); setSession(null); setError('Your session expired. Please sign in again.');
      } else {
        setRestoreFailed(true); setError('Unable to verify your session. Check the connection and retry.');
      }
    }).finally(() => { if (active) setRestoring(false); });
    return () => { active = false; };
  }, [restoreAttempt, invitationToken]);
  useEffect(() => {
    function unauthorized(event: Event) {
      if (session && (event as CustomEvent<string>).detail === session.token) {
        forgetToken(); setSession(null); setError('Your session expired. Please sign in again.');
      }
    }
    window.addEventListener('interview:unauthorized', unauthorized);
    return () => window.removeEventListener('interview:unauthorized', unauthorized);
  }, [session]);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('');
    const data = new FormData(event.currentTarget);
    try {
      const next = await request<Session>(`/api/auth/${register ? 'register' : 'login'}`, { method: 'POST', body: JSON.stringify(Object.fromEntries(data)) });
      rememberToken(next.token); setSession(next); setRestoreFailed(false);
    } catch (err) { setError(err instanceof Error ? err.message : 'Unable to sign in'); }
    finally { setBusy(false); }
  }
  async function logout() {
    if (!session) return;
    setBusy(true); setError('');
    try { await request<void>('/api/auth/logout', { method: 'POST' }, session.token); forgetToken(); setSession(null); }
    catch (error) { if (error instanceof ApiError && error.status === 401) { forgetToken(); setSession(null); } else { setError('Sign out failed. Please retry.'); } }
    finally { setBusy(false); }
  }
  if (invitationToken !== null) return <CandidateAccess key={invitationToken} token={invitationToken} />;
  if (restoring) return <main className="access-page"><section className="access-card" role="status"><h2>Restoring your session…</h2><p>Please wait while we verify your sign-in.</p></section></main>;
  if (restoreFailed) return <main className="access-page"><section className="access-card"><h2>Reconnect to your session</h2><p role="alert" className="error">{error}</p><button className="button" onClick={() => setRestoreAttempt(value => value + 1)}>Retry connection</button><button className="text-button" onClick={() => { forgetToken(); setRestoreFailed(false); setError(''); }}>Sign in with another account</button></section></main>;
  if (session) return <><Workspace scopeId={session.organization_id} token={session.token} onLogout={() => void logout()} />{error && <p className="toast error" role="alert">{error}</p>}</>;
  return <main className="access-page"><div className="access-intro"><a className="logo" href="#"><span className="logo-mark">i</span> interview /</a><p className="eyebrow">BETTER SIGNAL. BETTER HIRING.</p><h1>Great interviews.<br /><em>Grounded in evidence.</em></h1><p>Bring structure to your hiring process, from the first job description to the final review.</p><div className="access-features"><span>01 <strong>Define the role</strong></span><span>02 <strong>Invite your candidates</strong></span><span>03 <strong>Review with context</strong></span></div></div>
    <section className="access-card" aria-label="Recruiter access"><p className="eyebrow">RECRUITER & REVIEWER</p><h2>{register ? 'Create a recruiter account' : 'Welcome back'}</h2><p className="muted">Sign in to set up interviews and review candidate evidence.</p>{error && <p role="alert" className="error">{error}</p>}
      <form onSubmit={submit}><label>Email<input name="email" type="email" required maxLength={254} autoComplete="username" placeholder="you@company.com" /></label><label>Password<input name="password" type="password" required minLength={12} maxLength={128} autoComplete={register ? 'new-password' : 'current-password'} /></label><button className="button" disabled={busy}>{busy ? 'Please wait…' : register ? 'Create account →' : 'Sign in →'}</button><button type="button" className="text-button" disabled={busy} onClick={() => { setRegister(!register); setError(''); }}>{register ? 'Already have an account? Sign in' : 'New here? Create a recruiter account'}</button></form>
    </section></main>;
}
createRoot(document.getElementById('root')!).render(<StrictMode><App /></StrictMode>);
