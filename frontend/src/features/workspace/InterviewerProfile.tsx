import { LogOut, Mail, ShieldCheck, UserRound } from 'lucide-react';
import { Button } from '../../components/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '../../components/ui/card';
import { useEffect, useState } from 'react';
import { request } from '../../api';

export function useInterviewerAccount(token: string) {
  const [email, setEmail] = useState('');
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    request<{ email: string }>('/api/me', { signal: controller.signal }, token)
      .then(profile => {
        if (!controller.signal.aborted) setEmail(profile.email);
      })
      .catch(() => {
        if (!controller.signal.aborted) setError('Could not load your account details.');
      });
    return () => controller.abort();
  }, [token, attempt]);

  return { email, error, retry: () => { setError(''); setAttempt(value => value + 1); } };
}

export function InterviewerProfile({ account, onLogout }: { account: ReturnType<typeof useInterviewerAccount>; onLogout: () => void }) {
  const { email, error, retry } = account;
  return <div className="mx-auto grid max-w-3xl gap-6">
    <header>
      <p className="m-0 text-xs font-semibold tracking-widest text-indigo-700 uppercase workspace-dark:text-indigo-300">Account</p>
      <h1 className="my-2 font-display text-[2rem]">Profile</h1>
      <p className="m-0 text-sm text-slate-600 workspace-dark:text-slate-300">View your signed-in interviewer account.</p>
    </header>

    <Card aria-labelledby="profile-account">
      <CardHeader className="flex flex-wrap items-center gap-4">
        <span className="grid size-12 shrink-0 place-items-center rounded-xl bg-indigo-100 text-indigo-700 workspace-dark:bg-indigo-950 workspace-dark:text-indigo-300">
          <UserRound size={22} aria-hidden="true" />
        </span>
        <div>
          <h2 id="profile-account" className="m-0 text-xl">Account details</h2>
          <p className="m-0 text-sm text-slate-600 workspace-dark:text-slate-300">Identity and workspace access</p>
        </div>
      </CardHeader>
      <CardContent>
        <dl className="m-0 grid gap-5 rounded-lg bg-slate-50 p-4 sm:grid-cols-2 workspace-dark:bg-slate-800">
          <div className="min-w-0">
            <dt className="flex items-center gap-2 text-sm text-slate-600 workspace-dark:text-slate-300"><Mail size={16} aria-hidden="true" /> Email address</dt>
            <dd className="mt-2 ml-0 text-sm font-medium break-all" aria-live="polite">{email || (error ? 'Unavailable' : 'Loading…')}</dd>
          </div>
          <div>
            <dt className="flex items-center gap-2 text-sm text-slate-600 workspace-dark:text-slate-300"><ShieldCheck size={16} aria-hidden="true" /> Workspace role</dt>
            <dd className="mt-2 ml-0 inline-flex rounded-full bg-indigo-100 px-2.5 py-1 text-xs font-semibold text-indigo-800 workspace-dark:bg-indigo-950 workspace-dark:text-indigo-200">Interviewer</dd>
          </div>
        </dl>
        {error && <div className="mt-4 flex flex-wrap items-center justify-between gap-3 rounded-lg bg-rose-50 p-3 text-sm text-rose-900 workspace-dark:bg-rose-950 workspace-dark:text-rose-100">
          <p className="m-0" role="alert">{error}</p>
          <Button onClick={retry}>Retry</Button>
        </div>}
      </CardContent>
      <CardFooter className="justify-end">
        <Button onClick={onLogout}><LogOut size={16} aria-hidden="true" /> Sign out</Button>
      </CardFooter>
    </Card>
  </div>;
}
