import { Button } from '../../components/ui/button';
import { InterviewerProfile, useInterviewerAccount } from './InterviewerProfile';
import { LiveScreenViewer } from '../live-screen/LiveScreenViewer';
import { auditPresentation } from './auditPresentation';
import { ExpandableSection } from './ExpandableSection';
import { ArrowLeft, Home, Menu, Moon, Sun, UserRound, X } from 'lucide-react';
import { useWorkspaceTheme } from './useWorkspaceTheme';
import './workspace.css';
import { JobQuestionSets } from './JobQuestionSets';
import { RecordingEvidence } from './RecordingEvidence';
import { Analytics } from './Analytics';
import { ReviewAssistant } from './ReviewAssistant';
import { Assessment } from './Assessment';
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { QuestionPreparation } from './QuestionPreparation';
import { invitationIsActive, workspaceApi, type Audit, type AuditFilters, type AuditPage, type Interview, type Job, type Page, type Summary } from './api';

const reviewSections = [
  ['overview', 'Job & session'],
  ['questions', 'Questions & rubrics'],
  ['assessment', 'Assessment'],
  ['evidence', 'Evidence & assistant'],
  ['timeline', 'Session timeline'],
] as const;
type ReviewSection = typeof reviewSections[number][0];

const JOBS_PER_PAGE = 6;

type View = 'interviewer' | 'reviewer' | 'audit' | 'profile';
const message = (error: unknown) => error instanceof Error ? error.message : 'Something went wrong. Please retry.';
const date = (value: string | null) => value ? new Date(value).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : '—';
export function Status({ state }: { state: string }) { return <span className={`status status-${state}`}>{state.replaceAll('_', ' ')}</span>; }
function Timeline({ events, emptyMessage = "No audit events for this session yet." }: { events: Audit[]; emptyMessage?: string }) {
  if (!events.length) return <div className="empty compact">{emptyMessage}</div>;
  return <ol className="timeline">{events.map(event => {
    const { title, performedBy, details } = auditPresentation(event);
    return <li key={event.id}>
      <span className="timeline-dot" />
      <div className="audit-event-content">
        <strong>{title}</strong>
        <p><time dateTime={event.created_at}>{date(event.created_at)}</time> · {performedBy}</p>
        {(event.job_title || event.job_code) && <p className="audit-context">Job: {event.job_title}{event.job_code && <> · Job ID: {event.job_code}</>}</p>}
        {event.candidate_name && <p>Candidate: {event.candidate_name}{event.candidate_email && <> · {event.candidate_email}</>}</p>}
        {details.length > 0 && <p>{details.join(' · ')}</p>}
        <ExpandableSection className="audit-technical" label="Technical details">
          <p>Event ID: <code>{event.id}</code></p>
          {event.entity_id && <p>Related record ID: <code>{event.entity_id}</code></p>}
          {event.actor_id && <p>Actor ID: <code>{event.actor_id}</code></p>}
          <p>Action: <code>{event.action}</code></p>
        </ExpandableSection>
      </div>
    </li>;
  })}</ol>;
}
function Dialog({ children, title, onClose, busy = false }: { children: ReactNode; title: string; onClose: () => void; busy?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => { const dialog = ref.current; dialog?.showModal(); return () => dialog?.close(); }, []);
  return <dialog ref={ref} className="modal-backdrop interviewer-workspace" aria-labelledby={title} onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>{children}</dialog>;
}
export function Workspace({ token, onLogout }: { token: string; onLogout: () => void }) {
  const account = useInterviewerAccount(token);
  const accountMenu = useRef<HTMLDivElement>(null);
  const { theme, toggleTheme } = useWorkspaceTheme();
  const api = useMemo(() => workspaceApi(token), [token]);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const sidebarToggle = useRef<HTMLButtonElement>(null);
  const [viewHistory, setViewHistory] = useState<View[]>(['interviewer']);
  const view = viewHistory.at(-1) ?? 'interviewer';

  function navigateTo(next: View) {
    setViewHistory(history => history.at(-1) === next ? history : [...history, next]);
  }

  function goBack() {
    setViewHistory(history => history.length > 1 ? history.slice(0, -1) : history);
  }
  const [jobCursors, setJobCursors] = useState<string[]>([]);
  const [jobs, setJobs] = useState<Page<Job>>({ items: [], next_cursor: null });
  const [sessionCursors, setSessionCursors] = useState<string[]>([]);
  const [sessions, setSessions] = useState<Page<Interview>>({ items: [], next_cursor: null });
  const [audit, setAudit] = useState<AuditPage>({ items: [], next_cursor: null });
  const [auditCursors, setAuditCursors] = useState<string[]>([]);
  const auditScroll = useRef<HTMLDivElement>(null);
  const [auditForm, setAuditForm] = useState({ candidate: "", job: "", from: "", until: "" });
  const [auditFilters, setAuditFilters] = useState<AuditFilters>({});
  const [summary, setSummary] = useState<Summary>({ jobs: 0, sessions: 0, active: 0, reviewable: 0 });
  const [selected, setSelected] = useState<Interview | null>(null);
  const [events, setEvents] = useState<Page<Audit>>({ items: [], next_cursor: null });
  const [modal, setModal] = useState<'job' | 'invite' | null>(null);
  const [inviteJob, setInviteJob] = useState('');
  const [inviteDuration, setInviteDuration] = useState(30);
  const [analyticsJobId, setAnalyticsJobId] = useState<string | null>(null);
  const analyticsJob = jobs.items.find(job => job.id === analyticsJobId);
  const [expandedJobId, setExpandedJobId] = useState<string | null>(null);
  const expandedJob = jobs.items.find(job => job.id === expandedJobId);
  const [readyDurations, setReadyDurations] = useState<Record<string, number[]>>({});
  const updateReady = useCallback((job: string, durations: number[]) => setReadyDurations(previous => ({ ...previous, [job]: durations })), []);
  const [invitation, setInvitation] = useState('');
  const [detailLink, setDetailLink] = useState('');
  const [copyFeedback, setCopyFeedback] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [filter, setFilter] = useState('all');
  const [search, setSearch] = useState('');
  const [requestKey, setRequestKey] = useState(() => crypto.randomUUID());
  useEffect(() => {
    let active = true;
    Promise.all([api.jobs(undefined, JOBS_PER_PAGE), api.sessions(), api.audit(), api.summary()]).then(([j, s, a, counts]) => {
      if (active) { setJobCursors([]); setSessionCursors([]); setJobs(j); setSessions(s); setAudit(a); setSummary(counts); }
    }).catch(error => { if (active) setError(message(error)); }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [api]);
  function applyAuditFilters(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    const filters: AuditFilters = {};
    for (const key of ['candidate', 'job'] as const) {
      const value = String(form.get(key) ?? '').trim();
      if (value) filters[key] = value;
    }
    for (const key of ['from', 'until'] as const) {
      const value = String(form.get(key) ?? '');
      if (!value) continue;
      const day = new Date(`${value}T00:00:00`);
      if (key === 'until') day.setDate(day.getDate() + 1);
      filters[key] = String(day.getTime() / 1000);
    }
    void loadAudit(filters);
  }
  async function loadAudit(filters: AuditFilters) {
    await run(async () => {
      const page = await api.audit(undefined, undefined, filters);
      setAudit(page);
      setAuditFilters(filters);
      setAuditCursors([]);
      auditScroll.current?.scrollTo(0, 0);
    });
  }
  async function changeAuditPage(direction: 'previous' | 'next') {
    const cursor = audit.next_cursor;
    if (direction === 'next' && !cursor) return;
    const cursors = direction === 'next' && cursor
      ? [...auditCursors, cursor]
      : auditCursors.slice(0, -1);
    await run(async () => {
      const page = await api.audit(undefined, cursors.at(-1), auditFilters);
      setAudit(page);
      setAuditCursors(cursors);
      auditScroll.current?.scrollTo(0, 0);
    });
  }

  async function changeJobPage(direction: 'previous' | 'next') {
    const cursor = jobs.next_cursor;
    if (direction === 'next' && !cursor) return;
    const cursors = direction === 'next' && cursor
      ? [...jobCursors, cursor]
      : jobCursors.slice(0, -1);
    await run(async () => {
      const page = await api.jobs(cursors.at(-1), JOBS_PER_PAGE);
      setJobs(page);
      setJobCursors(cursors);
    });
  }
  const jobPagination = (
    <nav aria-label="Job pagination"
      className="flex flex-wrap items-center justify-between gap-3 border-t border-slate-200 px-4 py-3 sm:px-6 workspace-dark:border-slate-700">
      <Button disabled={busy || jobCursors.length === 0} onClick={() => void changeJobPage('previous')}>← Previous</Button>
      <span className="text-control tabular-nums" aria-live="polite">Page {jobCursors.length + 1} · {summary.jobs} {summary.jobs === 1 ? 'job' : 'jobs'} total</span>
      <Button disabled={busy || !jobs.next_cursor} onClick={() => void changeJobPage('next')}>Next →</Button>
    </nav>
  );
  async function changeSessionPage(direction: 'previous' | 'next') {
    const cursor = sessions.next_cursor;
    if (direction === 'next' && !cursor) return;
    const cursors = direction === 'next' && cursor
      ? [...sessionCursors, cursor]
      : sessionCursors.slice(0, -1);
    await run(async () => {
      const page = await api.sessions(cursors.at(-1));
      setSessions(page);
      setSessionCursors(cursors);
    });
  }
  async function refresh() {
    const [j, s, a, counts] = await Promise.all([api.jobs(undefined, JOBS_PER_PAGE), api.sessions(), api.audit(undefined, undefined, auditFilters), api.summary()]);
    setJobCursors([]);
    setSessionCursors([]);
    setJobs(j); setSessions(s); setAudit(a); setSummary(counts);
    setAuditCursors([]);
    auditScroll.current?.scrollTo(0, 0);
  }
  async function run(action: () => Promise<void>) {
    setBusy(true); setError('');
    try { await action(); } catch (error) { setError(message(error)); } finally { setBusy(false); }
  }
  function openModal(type: 'job' | 'invite', jobId = '') { const selected = jobId || jobs.items.find(job => readyDurations[job.id]?.length)?.id || ''; setInviteJob(selected); setInviteDuration(readyDurations[selected]?.[0] ?? 30); setRequestKey(crypto.randomUUID()); setInvitation(''); setModal(type); setError(''); }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); const form = new FormData(event.currentTarget);
    await run(async () => {
      if (modal === 'job') {
        await api.createJob({ question_set_count: Number(form.get('question_set_count')), preparation_duration_minutes: Number(form.get('preparation_duration_minutes')), job_code: String(form.get('job_code')), title: String(form.get('title')), description: String(form.get('description')), request_key: requestKey });
        setModal(null);
      } else {
        const result = await api.invite(String(form.get('job_id')), { candidate_name: String(form.get('candidate_name')), candidate_email: String(form.get('candidate_email')), duration_minutes: Number(form.get('duration_minutes')), request_key: requestKey });
        setInvitation(result.invitation_token ? `${window.location.origin}${window.location.pathname}#invite=${result.invitation_token}` : 'This older invitation already exists. Open its session details to recover or replace its link.');
      }
      await refresh();
    });
  }
  const [reviewSection, setReviewSection] = useState<ReviewSection>('overview');

  function inspect(id: string) {
    setReviewSection('overview');
    setDetailLink(''); setCopyFeedback('');
    void run(async () => {
      const [detail, history] = await Promise.all([api.detail(id), api.audit(id)]);
      setSelected(detail); setEvents(history);
      if (detail.invitation_recoverable && invitationIsActive(detail)) {
        const link = await api.invitation(id);
        setDetailLink(`${window.location.origin}${window.location.pathname}#invite=${link.invitation_token}`);
        setEvents(await api.audit(id));
      }
    });
  }
  function recoverInvitation() {
    if (!selected) return;
    void run(async () => {
      const result = await api.invitation(selected.id);
      setDetailLink(`${window.location.origin}${window.location.pathname}#invite=${result.invitation_token}`);
      setCopyFeedback(result.replaced_legacy_link ? 'Replacement created. The previous link no longer works.' : 'Saved invitation link ready to share.');
      setSelected({ ...selected, invitation_recoverable: true });
      setEvents(await api.audit(selected.id));
    });
  }
  async function copyInvitation() {
    try { await navigator.clipboard.writeText(detailLink); setCopyFeedback('Invitation link copied.'); }
    catch { setCopyFeedback('Select the link above and copy it manually.'); }
  }
  const visible = sessions.items.filter(s => (filter === 'all' || s.state === filter) && `${s.candidate_name} ${s.candidate_email} ${s.job_title} ${s.job_code}`.toLowerCase().includes(search.toLowerCase()));
  return <div className="interviewer-workspace flex min-h-screen [--sidebar-width:clamp(248px,18vw,350px)] bg-slate-50 text-slate-900 max-sm:flex-col workspace-dark:bg-slate-950 workspace-dark:text-slate-100" data-theme={theme}>
    <aside
      id="interviewer-sidebar"
      inert={!sidebarOpen}
      aria-hidden={!sidebarOpen}
      className={`flex flex-col overflow-hidden border-indigo-200 bg-indigo-100 sm:fixed sm:inset-y-0 sm:left-0 sm:z-10 sm:w-[var(--sidebar-width)] sm:overflow-y-auto sm:border-r sm:border-solid sm:px-4 sm:py-7 sm:shadow-sm 2xl:px-5 workspace-dark:border-slate-800 workspace-dark:bg-slate-900 motion-safe:transition-[translate,max-height,padding,opacity] motion-safe:duration-200 motion-safe:ease-out ${sidebarOpen
        ? 'max-sm:max-h-64 max-sm:border-b max-sm:border-solid max-sm:p-4 sm:translate-x-0'
        : 'max-sm:max-h-0 max-sm:px-4 max-sm:py-0 max-sm:opacity-0 sm:-translate-x-full'}`}
    >
      <div className="flex items-center justify-between gap-2 border-b border-indigo-200 pb-4 workspace-dark:border-slate-800">
        <a className="flex min-w-0 items-center gap-2 text-subheading font-semibold tracking-tight" href="#" onClick={event => { event.preventDefault(); navigateTo('interviewer'); }}><span aria-hidden="true" className="logo-mark shrink-0">i</span><span className="font-display text-subheading font-semibold tracking-tight uppercase text-primary workspace-dark:text-indigo-400">Interview</span><span className="text-indigo-500"> / </span></a>
        <Button aria-label="Close navigation" className="size-8 min-h-8 shrink-0 p-0" onClick={() => {
          setSidebarOpen(false);
          requestAnimationFrame(() => sidebarToggle.current?.focus());
        }}><X size={17} aria-hidden="true" /></Button>
      </div>
      <nav className="mt-5 flex min-h-0 flex-1 flex-col gap-2 max-sm:mt-3 max-sm:grid max-sm:grid-cols-3" aria-label="Interview navigation">
        {([
          ['interviewer', 'Jobs & invitations', 'Jobs'],
          ['reviewer', 'Interview results', 'Results'],
          ['audit', 'Audit trail', 'Audit'],
        ] as const).map(([key, label, shortLabel]) => <Button
          key={key}
          aria-label={label}
          aria-current={view === key ? 'page' : undefined}
          className="min-h-11 w-full min-w-0 justify-start px-3 py-2 text-left text-control aria-[current=page]:ring-1 aria-[current=page]:ring-focus max-sm:justify-center max-sm:px-1"
          onClick={() => navigateTo(key)}
        >
          <span className="hidden sm:inline-flex">{label}</span>
          <span className="sm:hidden">{shortLabel}</span>
        </Button>)}
      </nav>
    </aside>
    <div className={`flex min-w-0 flex-1 flex-col motion-safe:transition-[margin-left] motion-safe:duration-200 motion-safe:ease-out ${sidebarOpen ? 'sm:ml-[var(--sidebar-width)]' : ''}`}><header className="sticky top-0 z-10 flex h-16 shrink-0 items-center justify-between gap-2 border-b border-solid border-slate-200 bg-white px-4 text-control text-slate-700 sm:px-6 workspace-dark:border-slate-800 workspace-dark:bg-slate-900 workspace-dark:text-slate-200"><div className="flex min-w-0 items-center gap-3">
      {!sidebarOpen && <Button ref={sidebarToggle} aria-label="Open navigation" aria-controls="interviewer-sidebar" aria-expanded={false} className="size-9 min-h-9 shrink-0 p-0" onClick={() => {
        setSidebarOpen(true);
        requestAnimationFrame(() => document.querySelector<HTMLButtonElement>('#interviewer-sidebar button')?.focus());
      }}><Menu size={18} aria-hidden="true" /></Button>}
      <nav aria-label="Workspace navigation" className="flex shrink-0 gap-1">
        <Button type="button" aria-label="Go back" title="Back" disabled={viewHistory.length < 2}
          className="size-9 min-h-9 p-0" onClick={goBack}><ArrowLeft size={18} aria-hidden="true" /></Button>
        <Button type="button" aria-label="Go home" title="Home"
          className="size-9 min-h-9 p-0" onClick={() => navigateTo('interviewer')}><Home size={18} aria-hidden="true" /></Button>
      </nav>
      <span className="hidden truncate sm:inline">Interviews <span className="muted">/ {view === 'profile' ? 'Profile' : view === 'audit' ? 'Audit trail' : view === 'reviewer' ? 'Interview results' : 'Jobs & invitations'}</span></span></div><div className="flex shrink-0 items-center gap-2 sm:gap-4"><button type="button" role="switch" aria-label="Dark mode" aria-checked={theme === 'dark'} onClick={toggleTheme} className="group inline-flex min-h-10 items-center gap-2 rounded-lg border-0 bg-transparent px-2 text-slate-600 focus-visible:outline-2 focus-visible:outline-offset-2 workspace-dark:text-slate-300">
        <Sun size={16} aria-hidden="true" />
        <span aria-hidden="true" className="relative h-5 w-9 rounded-full bg-slate-400 transition-colors group-aria-checked:bg-primary motion-reduce:transition-none">
          <span className="absolute top-0.5 left-0.5 size-4 rounded-full bg-white shadow-sm transition-transform group-aria-checked:translate-x-4 motion-reduce:transition-none" />
        </span>
        <Moon size={16} aria-hidden="true" />
      </button><Button className="size-9 min-h-0 rounded-full p-0" aria-label="Account details" popoverTarget="interviewer-account"><UserRound size={17} aria-hidden="true" /></Button>
        <div ref={accountMenu} id="interviewer-account" popover="auto" aria-label="Account details" className="fixed inset-auto top-16 right-4 m-0 w-72 max-w-[calc(100vw-2rem)] rounded-xl border border-solid border-slate-200 bg-white p-4 text-slate-900 shadow-lg workspace-dark:border-slate-700 workspace-dark:bg-slate-900 workspace-dark:text-slate-100">
          <p className="m-0 text-detail font-semibold uppercase tracking-wide">Interviewer account</p>
          <p className="mt-2 mb-4 break-all text-control" aria-live="polite">{account.email || (account.error ? 'Email unavailable' : 'Loading email…')}</p>
          <div className="flex flex-wrap gap-2">
            <Button onClick={() => { accountMenu.current?.hidePopover(); navigateTo('profile'); }}>View profile</Button>
            <Button onClick={() => { accountMenu.current?.hidePopover(); onLogout(); }}>Sign out</Button>
          </div>
        </div></div></header>
      <main className="dashboard flex w-full flex-1 flex-col">{view === 'profile' ? <InterviewerProfile account={account} onLogout={onLogout} /> : <><div className="page-heading"><div><h1 className="font-display">{view === 'reviewer' ? 'Interview results' : view === 'audit' ? 'Audit trail' : 'Jobs & invitations'}</h1></div><div className="actions"><button className="button secondary" disabled={busy || loading} onClick={() => void run(refresh)}>↻ Refresh</button>{view === 'interviewer' && <button className="button" disabled={busy || loading} onClick={() => openModal('job')}>+ Create job</button>}</div></div>
        {error && <div className="error" role="alert">{error}</div>}
        {view === 'reviewer' && <div className="stats">{[['Total roles', summary.jobs, 'Job descriptions created'], ['Invitations', summary.sessions, 'Across all your roles'], ['Active sessions', summary.active, 'Ready or in progress'], ['Ready for review', summary.reviewable, 'Submitted, scoring, or completed']].map(([label, value, help]) => <div className="stat" key={label}><span>{label}</span><strong>{loading ? '—' : value}</strong><small>{help}</small></div>)}</div>}
        {loading ? <section className="panel empty" role="status">Loading your interviews…</section> : view === 'audit' ? <section className="panel pt-6" aria-label="Audit events">
          <form className="audit-filters px-6 max-sm:px-4" onSubmit={applyAuditFilters} onReset={() => { setAuditForm({ candidate: "", job: "", from: "", until: "" }); void loadAudit({}); }}>
            <label>Candidate<input name="candidate" value={auditForm.candidate} onChange={event => setAuditForm({ ...auditForm, candidate: event.target.value })} placeholder="Name or email" /></label>
            <label>Job<input name="job" value={auditForm.job} onChange={event => setAuditForm({ ...auditForm, job: event.target.value })} placeholder="Title or job ID" /></label>
            <label>From date<input type="date" name="from" value={auditForm.from} onChange={event => setAuditForm({ ...auditForm, from: event.target.value })} /></label>
            <label>To date<input type="date" name="until" value={auditForm.until} onChange={event => setAuditForm({ ...auditForm, until: event.target.value })} /></label>
            <div className="audit-filter-actions">
              <button type="submit" className="button primary" disabled={busy}>Apply filters</button>
              <button type="reset" className="button" disabled={busy}>Clear</button>
            </div>
          </form>
          <div ref={auditScroll} className="audit-scroll" role="region" aria-label="Audit events" tabIndex={0}>
            {audit.items.length === 0 && (auditFilters.from || auditFilters.until) ? (
              <div className="empty compact">
                <p>No audit events match the selected dates and search filters.</p>
                <Button disabled={busy} onClick={() => {
                  setAuditForm(form => ({ ...form, from: '', until: '' }));
                  const filters: AuditFilters = {};
                  if (auditFilters.candidate) filters.candidate = auditFilters.candidate;
                  if (auditFilters.job) filters.job = auditFilters.job;
                  void loadAudit(filters);
                }}>Search all dates</Button>
              </div>
            ) : <Timeline events={audit.items} emptyMessage="No audit events match these filters." />}
          </div>
          <nav aria-label="Audit pagination"
            className="flex flex-wrap items-center justify-between gap-3 border-t border-slate-200 px-4 py-3 sm:px-6 workspace-dark:border-slate-700">
            <Button disabled={busy || auditCursors.length === 0} onClick={() => void changeAuditPage('previous')}>← Previous</Button>
            <span className="text-control tabular-nums" aria-live="polite">Page {auditCursors.length + 1}{audit.total !== undefined && ` · ${audit.total} matching ${audit.total === 1 ? 'event' : 'events'}`}</span>
            <Button disabled={busy || !audit.next_cursor} onClick={() => void changeAuditPage('next')}>Next →</Button>
          </nav>
        </section> : <>
          {view === 'interviewer' && <section className="panel" aria-label="Your roles">{jobs.items.length ? <div className="job-grid items-stretch! pt-6">
            {jobs.items.map(job => (
              <article key={job.id} className="job-card group relative gap-3 hover:border-indigo-400! focus-within:border-indigo-400! motion-safe:transition-[transform,box-shadow,border-color]! motion-safe:hover:-translate-y-1">
                <div className="flex items-start justify-between gap-3">
                  <span className="pill max-w-full break-all">{job.job_code}</span>
                  <span aria-hidden="true" className="job-icon grid shrink-0 place-items-center motion-safe:transition-transform motion-safe:group-hover:translate-x-0.5 motion-safe:group-hover:-translate-y-0.5">↗</span>
                </div>
                <h3 className="mt-0! break-words">{job.title}</h3>
                <div className="mt-auto border-t border-slate-200 pt-3 workspace-dark:border-slate-700">
                  {expandedJobId === job.id
                    ? <span className="text-control">Question sets</span>
                    : <JobQuestionSets key={job.session_count} job={job.id} token={token} onReadyChange={updateReady} summaryOnly />}
                </div>
                <button type="button" className="absolute inset-0 cursor-pointer rounded-xl border-0 bg-transparent p-0 hover:bg-transparent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500"
                  aria-label={`Open job details for ${job.job_code}: ${job.title}`} aria-haspopup="dialog"
                  onClick={() => setExpandedJobId(job.id)} />
              </article>
            ))}
          </div> : <div className="empty"><div className="empty-icon">▧</div><h3>Your first interview starts here</h3><p>Add a job description to create your first role.</p><button className="button" onClick={() => openModal('job')}>+ Create your first job</button></div>}{jobPagination}</section>}
          {view === 'reviewer' && <section className="panel" aria-label="Candidate interviews"><div className="table-toolbar"><input aria-label="Search interviews on this page" placeholder="Search this page…" value={search} onChange={e => setSearch(e.target.value)} /><select aria-label="Filter interview status on this page" value={filter} onChange={e => setFilter(e.target.value)}><option value="all">All statuses</option>{['invited', 'ready', 'in_progress', 'submitted', 'scoring', 'completed', 'expired'].map(s => <option value={s} key={s}>{s.replaceAll('_', ' ')}</option>)}</select></div>
            {visible.length ? <div className="table-scroll"><table><thead><tr><th>Candidate</th><th>Role</th><th>Status</th><th>Duration</th><th>Invited</th><th><span className="sr-only">Actions</span></th></tr></thead><tbody>{visible.map(s => <tr key={s.id}><td><strong>{s.candidate_name}</strong><small>{s.candidate_email}</small></td><td>{s.job_title}<small>Job ID: {s.job_code}</small></td><td><Status state={s.state} /></td><td>{s.duration_minutes} min</td><td>{date(s.created_at)}</td><td><button className="text-button accent" disabled={busy} onClick={() => inspect(s.id)}>View →</button></td></tr>)}</tbody></table></div> : <div className="empty compact"><h3>{sessions.items.length ? 'No matching sessions on this page' : 'No candidates yet'}</h3><p>{sessions.items.length ? 'Try another search or status.' : 'Invite a candidate once a question set is ready. Their progress and results will appear here.'}</p></div>}
            <nav aria-label="Interview pagination"
              className="flex flex-wrap items-center justify-between gap-3 border-t border-slate-200 px-4 py-3 sm:px-6 workspace-dark:border-slate-700">
              <Button disabled={busy || sessionCursors.length === 0} onClick={() => void changeSessionPage('previous')}>← Previous</Button>
              <span className="text-control tabular-nums" aria-live="polite">Page {sessionCursors.length + 1} · {summary.sessions} {summary.sessions === 1 ? 'interview' : 'interviews'} total</span>
              <Button disabled={busy || !sessions.next_cursor} onClick={() => void changeSessionPage('next')}>Next →</Button>
            </nav></section>}{view === 'reviewer' && jobs.items.length > 0 && <section className="panel">
            <div className="panel-heading"><h2>Question-bank analytics</h2></div>
            <div className="job-grid items-stretch!">{jobs.items.map(job => <article className="job-card" key={job.id}>
              <div className="flex items-start justify-between gap-3">
                <h3 className="min-w-0 break-words mt-0!">{job.title}</h3>
                <Button className="job-icon min-h-0 shrink-0 p-0" aria-label={`Open analytics for ${job.title}`}
                  aria-haspopup="dialog" onClick={() => setAnalyticsJobId(job.id)}>↗</Button>
              </div>
              <p className="job-code">Job ID: <strong>{job.job_code}</strong></p>
              <p className="job-description">{job.description}</p>
              <button type="button" className="text-button accent" aria-haspopup="dialog"
                onClick={() => setAnalyticsJobId(job.id)}>View analytics →</button>
            </article>)}</div>
            {jobPagination}
          </section>}</>}
      </>}<footer className="mt-auto">INTERVIEW PLATFORM <span>Structured interviews · Evidence-grounded review</span></footer></main></div>
    {analyticsJob && <Dialog title="analytics-title" onClose={() => setAnalyticsJobId(null)}>
      <section className="modal detail-modal w-[min(1200px,calc(100vw-2rem))]!">
        <header className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <p className="eyebrow">QUESTION PERFORMANCE</p>
            <h2 id="analytics-title" className="break-words">{analyticsJob.title}</h2>
            <p className="job-code">Job ID: <strong>{analyticsJob.job_code}</strong></p>
          </div>
          <Button className="icon-button shrink-0" aria-label="Close analytics" onClick={() => setAnalyticsJobId(null)}>×</Button>
        </header>
        <Analytics job={analyticsJob.id} token={token} />
      </section>
    </Dialog>}
    {expandedJob && <Dialog title="job-details-title" onClose={() => setExpandedJobId(null)}>
      <section className="modal detail-modal">
        <div className="panel-heading">
          <div className="min-w-0">
            <p className="eyebrow">JOB DETAILS</p>
            <h2 id="job-details-title" className="break-words">{expandedJob.title}</h2>
            <p>Job ID {expandedJob.job_code} · {expandedJob.session_count} {expandedJob.session_count === 1 ? 'invitation' : 'invitations'}</p>
          </div>
          <Button type="button" className="icon-button shrink-0" aria-label="Close job details" onClick={() => setExpandedJobId(null)}>×</Button>
        </div>
        <div className="my-6">
          <h3>Job description</h3>
          <p className="preserve max-w-prose break-words">{expandedJob.description}</p>
        </div>
        <JobQuestionSets job={expandedJob.id} token={token} onReadyChange={updateReady} />
        <div className="mt-6 flex flex-wrap items-center gap-3">
          <Button disabled={!readyDurations[expandedJob.id]?.length} onClick={() => {
            setExpandedJobId(null);
            openModal('invite', expandedJob.id);
          }}>Invite a candidate</Button>
          {!readyDurations[expandedJob.id]?.length && <small className="muted">Invitations unlock when a question set is complete.</small>}
        </div>
      </section>
    </Dialog>}
    {modal && <Dialog title="modal-title" onClose={() => setModal(null)} busy={busy}><section className="modal"><div className="panel-heading"><h2 id="modal-title">{modal === 'job' ? 'Create a job' : invitation ? 'Invitation created' : 'Invite a candidate'}</h2><button className="icon-button" aria-label="Close dialog" disabled={busy} onClick={() => setModal(null)}>×</button></div>{error && <p className="error" role="alert">{error}</p>}{invitation ? <><p>Share this private link with the candidate. The link grants access to this session and expires in seven days.</p><label>Candidate invitation link<textarea readOnly rows={4} value={invitation} onFocus={e => e.currentTarget.select()} /></label><p className="muted">This link is saved securely. You can retrieve it again from the session details to share later.</p><button className="button" onClick={() => setModal(null)}>Done</button></> : <form onSubmit={save} onChange={() => setRequestKey(crypto.randomUUID())}>{modal === 'job' ? <><label>Job ID<input name="job_code" required maxLength={64} placeholder="e.g. BE-2026-001" autoComplete="off" aria-describedby="job-code-help" /></label><p id="job-code-help" className="muted">Unique for your account, ignoring letter case. Start with a letter or number; use letters, numbers, hyphens or underscores.</p><label>Job title<input name="title" required maxLength={160} placeholder="e.g. Senior Backend Engineer" /></label><label>Job description<textarea name="description" required minLength={20} maxLength={20000} rows={8} placeholder="Describe the role, responsibilities, and required skills…" /></label><label>Interview duration (minutes)<input name="preparation_duration_minutes" type="number" min={5} max={180} defaultValue={30} required /></label><label>Number of question sets<input name="question_set_count" type="number" min={1} max={5} defaultValue={1} required /></label><p className="muted">Prepare 1–5 unique sets with reference answers. Each new question is scoped to 2–3 minutes; short interviews use fewer questions. More sets use more AI requests. Only this many sets are created and reused across candidates. No automatic replacements.</p></> : <><label>Role<select name="job_id" defaultValue={inviteJob || jobs.items[0]?.id} onChange={event => { setInviteJob(event.target.value); setInviteDuration(readyDurations[event.target.value]?.[0] ?? 30); }} required>{jobs.items.map(job => <option disabled={!readyDurations[job.id]?.length} key={job.id} value={job.id}>{job.job_code} — {job.title}</option>)}</select></label><label>Candidate name<input name="candidate_name" required maxLength={120} autoComplete="name" /></label><label>Candidate email<input name="candidate_email" type="email" required maxLength={254} autoComplete="email" /></label><label>Interview duration (minutes)<input key={inviteJob} name="duration_minutes" type="number" min={5} max={180} value={inviteDuration} onChange={event => setInviteDuration(Number(event.target.value))} required /></label><p className="muted">Create a private link to share manually. Email delivery is not connected yet.</p>{!readyDurations[inviteJob]?.includes(inviteDuration) && <p className="configuration-note">No completed set is available for this duration. Choose a duration with a ready set before inviting.</p>}</>}<button className="button" disabled={busy || (modal === 'invite' && !readyDurations[inviteJob]?.includes(inviteDuration))}>{busy ? 'Saving…' : modal === 'job' ? 'Create job' : 'Create invitation'}</button></form>}</section></Dialog>}
    {selected && <Dialog title="detail-title" onClose={() => setSelected(null)}><section className="modal detail-modal flex! h-[88dvh] w-[min(1280px,calc(100vw-2rem))]! flex-col overflow-hidden! p-0!"><div className="flex shrink-0 items-center justify-between gap-4 border-b border-slate-200 px-5 py-4 sm:px-6 workspace-dark:border-slate-700"><div className="min-w-0"><p className="eyebrow m-0!">SESSION DETAILS</p><h2 id="detail-title" className="mt-1 truncate text-section-title font-semibold">{selected.candidate_name}</h2><p className="m-0! truncate text-control text-slate-500 workspace-dark:text-slate-400">{selected.candidate_email}</p></div><button className="icon-button shrink-0" aria-label="Close session details" onClick={() => setSelected(null)}>×</button></div><div className="grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)] md:grid-cols-[13rem_minmax(0,1fr)] md:grid-rows-[minmax(0,1fr)]">
      <nav aria-label="Session sections" className="flex min-w-0 gap-1 overflow-x-auto border-b border-slate-200 bg-slate-50 p-3 md:flex-col md:border-r md:border-b-0 workspace-dark:border-slate-700 workspace-dark:bg-slate-900">
        {reviewSections.map(([id, label]) => <button
          key={id}
          type="button"
          aria-current={reviewSection === id ? 'true' : undefined}
          onClick={() => { setReviewSection(id); document.getElementById('session-review-content')?.scrollTo(0, 0); }}
          className="shrink-0 whitespace-nowrap rounded-md border-0 bg-transparent px-3 py-2 text-left text-control font-medium text-slate-600 hover:bg-slate-100 workspace-dark:text-slate-300 workspace-dark:hover:bg-slate-800 aria-[current=true]:bg-indigo-50 aria-[current=true]:text-indigo-700 workspace-dark:aria-[current=true]:bg-indigo-950 workspace-dark:aria-[current=true]:text-indigo-300"
        >{label}</button>)}
      </nav>
      <div id="session-review-content" role="region" aria-label="Selected session section" tabIndex={0} className="min-h-0 min-w-0 overflow-y-auto overscroll-contain [scrollbar-gutter:stable] p-4 sm:p-6 [&>section>.question-preparation:first-child]:mt-0!">
      <section hidden={reviewSection !== 'overview'} aria-label="Job and session">
      <Status state={selected.state} /><h3>{selected.job_title}</h3><p className="job-code">Job ID: <strong>{selected.job_code}</strong></p><div className="detail-grid"><div><small>Duration</small><strong>{selected.duration_minutes} minutes</strong></div><div><small>Invitation expires</small><strong>{date(selected.expires_at)}</strong></div><div><small>Started</small><strong>{date(selected.started_at)}</strong></div><div><small>Submitted</small><strong>{date(selected.submitted_at)}</strong></div></div>{error && <p className="error" role="alert">{error}</p>}{invitationIsActive(selected) && <div className="invitation-panel"><h3>Candidate invitation</h3>{selected.invitation_recoverable === false && <p>This older invitation does not have a saved link. Create a replacement to share; the previous link will stop working.</p>}{detailLink ? <><label>Saved invitation link<textarea readOnly rows={3} value={detailLink} onFocus={e => e.currentTarget.select()} /></label><button className="button" onClick={() => void copyInvitation()}>Copy invitation link</button></> : <><p>Retrieve the private link to share with the candidate again. Its original expiry stays the same.</p><button className="button" disabled={busy || (selected.invitation_recoverable === false && selected.state !== 'invited')} onClick={recoverInvitation}>{busy ? 'Retrieving…' : selected.invitation_recoverable === false ? 'Create replacement link' : 'Show invitation link'}</button>{selected.invitation_recoverable === false && selected.state !== 'invited' && <p>This older session is already accepted. Continue using its original link.</p>}</>}{copyFeedback && <p role="status" className="invitation-feedback">{copyFeedback}</p>}</div>}<ExpandableSection label={<>Job requirements</>}><p className="preserve">{selected.description}</p></ExpandableSection>
      </section>
      <section hidden={reviewSection !== 'questions'} aria-label="Questions and rubrics">
        <QuestionPreparation key={`questions:${selected.id}`} interview={selected} token={token} />
      </section>
      <section hidden={reviewSection !== 'assessment'} aria-label="Assessment">
        <Assessment session={selected.id} token={token} />
      </section>
      <section hidden={reviewSection !== 'evidence'} aria-label="Evidence and assistant">
      {selected.state === 'in_progress' && <LiveScreenViewer key={`live:${selected.id}`} session={selected.id} token={token} />}<RecordingEvidence session={selected.id} token={token} /><ReviewAssistant key={`review:${selected.id}`} session={selected.id} token={token} /></section>
      <section hidden={reviewSection !== 'timeline'} aria-label="Session timeline">
      <ExpandableSection className="session-timeline" label={<>Session timeline ({events.items.length}) </>} help="Saved audit history for this session. Expand it and use Load older events to browse earlier activity."><Timeline events={events.items} />{events.next_cursor && <button className="load-more" disabled={busy} onClick={() => void run(async () => { const next = await api.audit(selected.id, events.next_cursor!); setEvents({ items: [...events.items, ...next.items], next_cursor: next.next_cursor }); })}>Load older events ↓</button>}</ExpandableSection><p className="muted">Session ID: <code>{selected.id}</code></p></section></div></div></section></Dialog>}
  </div>;
}
