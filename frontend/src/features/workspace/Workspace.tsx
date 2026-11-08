import { auditPresentation } from './auditPresentation';
import { ExpandableSection } from './ExpandableSection';
import { Moon, Sun } from 'lucide-react';
import { useWorkspaceTheme } from './useWorkspaceTheme';
import './workspace.css';
import { JobQuestionSets } from './JobQuestionSets';
import { RecordingEvidence } from './RecordingEvidence';
import { Analytics } from './Analytics';
import { ReviewAssistant } from './ReviewAssistant';
import { Assessment } from './Assessment';
import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { QuestionPreparation } from './QuestionPreparation';
import { invitationIsActive, workspaceApi, type Audit, type Interview, type Job, type Page, type Summary } from './api';

type View = 'interviewer' | 'reviewer' | 'audit';
const message = (error: unknown) => error instanceof Error ? error.message : 'Something went wrong. Please retry.';
const date = (value: string | null) => value ? new Date(value).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : '—';
export function Status({ state }: { state: string }) { return <span className={`status status-${state}`}>{state.replaceAll('_', ' ')}</span>; }
function Timeline({ events }: { events: Audit[] }) {
  if (!events.length) return <div className="empty compact">No audit events for this session yet.</div>;
  return <ol className="timeline">{events.map(event => {
    const { title, performedBy, details } = auditPresentation(event);
    return <li key={event.id}>
      <span className="timeline-dot" />
      <div className="audit-event-content">
        <strong>{title}</strong>
        <p>{date(event.created_at)} · {performedBy}</p>
        {(event.job_title || event.job_code) && <p className="audit-context">{event.job_title}{event.job_code && <> · Job ID: {event.job_code}</>}</p>}
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
  const { theme, toggleTheme } = useWorkspaceTheme();
  const api = useMemo(() => workspaceApi(token), [token]);
  const [view, setView] = useState<View>('interviewer');
  const [jobs, setJobs] = useState<Page<Job>>({ items: [], next_cursor: null });
  const [sessions, setSessions] = useState<Page<Interview>>({ items: [], next_cursor: null });
  const [audit, setAudit] = useState<Page<Audit>>({ items: [], next_cursor: null });
  const [summary, setSummary] = useState<Summary>({ jobs: 0, sessions: 0, active: 0, reviewable: 0 });
  const [selected, setSelected] = useState<Interview | null>(null);
  const [events, setEvents] = useState<Page<Audit>>({ items: [], next_cursor: null });
  const [modal, setModal] = useState<'job' | 'invite' | null>(null);
  const [inviteJob, setInviteJob] = useState('');
  const [inviteDuration, setInviteDuration] = useState(30);
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
    Promise.all([api.jobs(), api.sessions(), api.audit(), api.summary()]).then(([j, s, a, counts]) => {
      if (active) { setJobs(j); setSessions(s); setAudit(a); setSummary(counts); }
    }).catch(error => { if (active) setError(message(error)); }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [api]);
  async function refresh() {
    const [j, s, a, counts] = await Promise.all([api.jobs(), api.sessions(), api.audit(), api.summary()]);
    setJobs(j); setSessions(s); setAudit(a); setSummary(counts);
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
  function inspect(id: string) {
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
  return <div className="app-shell interviewer-workspace" data-theme={theme}>
    <aside className="sidebar"><a className="logo" href="#"><span className="logo-mark">i</span> interview<span className="logo-light"> / </span></a><div className="workspace-label">INTERVIEW PLATFORM</div><div className="workspace-name">Interviewer<small>Candidate assessment</small></div>
      <nav aria-label="Interview navigation"><button aria-current={view === 'interviewer' ? 'page' : undefined} className={view === 'interviewer' ? 'nav-active' : ''} onClick={() => setView('interviewer')}><span>▦</span> Jobs & invitations</button><button aria-current={view === 'reviewer' ? 'page' : undefined} className={view === 'reviewer' ? 'nav-active' : ''} onClick={() => setView('reviewer')}><span>◎</span> Interview results</button><button aria-current={view === 'audit' ? 'page' : undefined} className={view === 'audit' ? 'nav-active' : ''} onClick={() => setView('audit')}><span>≡</span> Audit trail</button></nav>
      <div className="sidebar-bottom"><div className="foundation-tag">INTERVIEWS <span>Evidence first</span></div><p>Structured interviews.<br />Evidence at every step.</p><button className="text-button" onClick={onLogout}>Sign out ↗</button></div></aside>
    <div className="workspace-main"><header className="topbar"><span>Interviews <span className="muted">/ {view === 'audit' ? 'Audit trail' : view === 'reviewer' ? 'Interview results' : 'Jobs & invitations'}</span></span><div className="flex items-center gap-4"><button type="button" className="theme-toggle" aria-label={theme === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'} onClick={toggleTheme}>{theme === 'dark' ? <Sun size={17} aria-hidden="true" /> : <Moon size={17} aria-hidden="true" />}<span>{theme === 'dark' ? 'Light mode' : 'Dark mode'}</span></button><span className="avatar" title="Interviewer">IN</span></div></header>
      <main className="dashboard"><div className="page-heading"><div><p className="eyebrow">{view === 'reviewer' ? 'CANDIDATE ASSESSMENT' : view === 'audit' ? 'INTERVIEW HISTORY' : 'INTERVIEW OPERATIONS'}</p><h1>{view === 'reviewer' ? 'Interview results' : view === 'audit' ? 'Audit trail' : 'Jobs & invitations'}</h1><p className="muted">{view === 'reviewer' ? 'Inspect interview status, job requirements, and recorded activity.' : view === 'audit' ? 'Trace interview actions back to their time, actor, and session.' : 'Create a role, invite candidates, and keep interviews moving.'}</p></div><div className="actions"><button className="button secondary" disabled={busy || loading} onClick={() => void run(refresh)}>↻ Refresh</button>{view === 'interviewer' && <button className="button" disabled={busy || loading} onClick={() => openModal('job')}>+ Create job</button>}</div></div>
      {error && <div className="error" role="alert">{error}</div>}
      <div className="stats">{[['Total roles', summary.jobs, 'Job descriptions created'], ['Invitations', summary.sessions, 'Across all your roles'], ['Active sessions', summary.active, 'Ready or in progress'], ['Ready for review', summary.reviewable, 'Submitted, scoring, or completed']].map(([label, value, help]) => <div className="stat" key={label}><span>{label}</span><strong>{loading ? '—' : value}</strong><small>{help}</small></div>)}</div>
      {loading ? <section className="panel empty" role="status">Loading your interviews…</section> : view === 'audit' ? <section className="panel"><div className="panel-heading"><div><h2>Audit trail</h2><p>Append-only records of interview activity</p></div><span className="pill">Evidence log</span></div><div className="audit-scroll" role="region" aria-label="Audit events" tabIndex={0}><Timeline events={audit.items} /></div>{audit.next_cursor && <button className="load-more" disabled={busy} onClick={() => void run(async () => { const next = await api.audit(undefined, audit.next_cursor!); setAudit({ items: [...audit.items, ...next.items], next_cursor: next.next_cursor }); })}>Load older events ↓</button>}</section> : <>
      {view === 'interviewer' && <section className="panel"><div className="panel-heading"><div><h2>Your roles</h2><p>Each interview starts with a clear job description.</p></div><span className="pill">{summary.jobs} total</span></div>{jobs.items.length ? <div className="job-grid">{jobs.items.map(job => <article className="job-card" key={job.id}><div className="job-icon">↗</div><h3>{job.title}</h3><p className="job-code">Job ID: <strong>{job.job_code}</strong></p><p className="job-description">{job.description}</p><div className="job-meta"><span>Job description</span><span>{job.session_count} invitation{job.session_count !== 1 ? 's' : ''}</span></div><ExpandableSection label={<>Read job description</>}><p className="preserve">{job.description}</p></ExpandableSection><JobQuestionSets key={`${job.id}:${job.session_count}`} job={job.id} token={token} onReadyChange={updateReady} /><Analytics job={job.id} token={token} /><button className="text-button accent" disabled={!readyDurations[job.id]?.length} onClick={() => openModal('invite', job.id)}>Invite a candidate →</button>{!readyDurations[job.id]?.length && <small className="muted">Invitations unlock when a question set is complete.</small>}</article>)}</div> : <div className="empty"><div className="empty-icon">▧</div><h3>Your first interview starts here</h3><p>Add a job description to create your first role.</p><button className="button" onClick={() => openModal('job')}>+ Create your first job</button></div>}{jobs.next_cursor && <button className="load-more" disabled={busy} onClick={() => void run(async () => { const next = await api.jobs(jobs.next_cursor!); setJobs({ items: [...jobs.items, ...next.items], next_cursor: next.next_cursor }); })}>Load more roles ↓</button>}</section>}
      <section className="panel"><div className="panel-heading"><div><h2>{view === 'reviewer' ? 'Candidate review queue' : 'Interview sessions'}</h2><p>{view === 'reviewer' ? 'Open a candidate to inspect their session and evidence.' : 'Follow every invitation from preparation to completion.'}</p></div><button className="button secondary" disabled={!jobs.items.some(job => readyDurations[job.id]?.length)} onClick={() => openModal('invite')}>+ Invite candidate</button></div><div className="table-toolbar"><input aria-label="Search loaded sessions" placeholder="Search candidates, roles or Job IDs…" value={search} onChange={e => setSearch(e.target.value)} /><select aria-label="Filter session status" value={filter} onChange={e => setFilter(e.target.value)}><option value="all">All statuses</option>{['invited', 'ready', 'in_progress', 'submitted', 'scoring', 'completed', 'expired'].map(s => <option value={s} key={s}>{s.replaceAll('_', ' ')}</option>)}</select></div>
      {visible.length ? <div className="table-scroll"><table><thead><tr><th>Candidate</th><th>Role</th><th>Status</th><th>Duration</th><th>Invited</th><th><span className="sr-only">Actions</span></th></tr></thead><tbody>{visible.map(s => <tr key={s.id}><td><strong>{s.candidate_name}</strong><small>{s.candidate_email}</small></td><td>{s.job_title}<small>Job ID: {s.job_code}</small></td><td><Status state={s.state} /></td><td>{s.duration_minutes} min</td><td>{date(s.created_at)}</td><td><button className="text-button accent" disabled={busy} onClick={() => inspect(s.id)}>View →</button></td></tr>)}</tbody></table></div> : <div className="empty compact"><h3>{sessions.items.length ? 'No matching sessions' : 'No candidates yet'}</h3><p>{sessions.items.length ? 'Try another search or status.' : 'Invite a candidate once a question set is ready. Their progress and results will appear here.'}</p></div>}
      {sessions.next_cursor && <button className="load-more" disabled={busy} onClick={() => void run(async () => { const next = await api.sessions(sessions.next_cursor!); setSessions({ items: [...sessions.items, ...next.items], next_cursor: next.next_cursor }); })}>Load more sessions ↓</button>}</section><div className="phase-note"><span>◈</span><p><strong>A solid foundation for evidence-based interviews.</strong> Prepare reusable question sets for each role. Review candidate answers, rubric scores and recorded evidence.</p></div></>}
      <footer>INTERVIEW PLATFORM <span>Structured interviews · Evidence-grounded review</span></footer></main></div>
    {modal && <Dialog title="modal-title" onClose={() => setModal(null)} busy={busy}><section className="modal"><div className="panel-heading"><h2 id="modal-title">{modal === 'job' ? 'Create a job' : invitation ? 'Invitation created' : 'Invite a candidate'}</h2><button className="icon-button" aria-label="Close dialog" disabled={busy} onClick={() => setModal(null)}>×</button></div>{error && <p className="error" role="alert">{error}</p>}{invitation ? <><p>Share this private link with the candidate. The link grants access to this session and expires in seven days.</p><label>Candidate invitation link<textarea readOnly rows={4} value={invitation} onFocus={e => e.currentTarget.select()} /></label><p className="muted">This link is saved securely. You can retrieve it again from the session details to share later.</p><button className="button" onClick={() => setModal(null)}>Done</button></> : <form onSubmit={save} onChange={() => setRequestKey(crypto.randomUUID())}>{modal === 'job' ? <><label>Job ID<input name="job_code" required maxLength={64} placeholder="e.g. BE-2026-001" autoComplete="off" aria-describedby="job-code-help" /></label><p id="job-code-help" className="muted">Unique for your account, ignoring letter case. Start with a letter or number; use letters, numbers, hyphens or underscores.</p><label>Job title<input name="title" required maxLength={160} placeholder="e.g. Senior Backend Engineer" /></label><label>Job description<textarea name="description" required minLength={20} maxLength={20000} rows={8} placeholder="Describe the role, responsibilities, and required skills…" /></label><label>Interview duration (minutes)<input name="preparation_duration_minutes" type="number" min={5} max={180} defaultValue={30} required /></label><label>Number of question sets<input name="question_set_count" type="number" min={1} max={5} defaultValue={1} required /></label><p className="muted">Prepare 1–5 unique sets with reference answers. Each new question is scoped to 2–3 minutes; short interviews use fewer questions. More sets use more AI requests. Only this many sets are created and reused across candidates. No automatic replacements.</p></> : <><label>Role<select name="job_id" defaultValue={inviteJob || jobs.items[0]?.id} onChange={event => { setInviteJob(event.target.value); setInviteDuration(readyDurations[event.target.value]?.[0] ?? 30); }} required>{jobs.items.map(job => <option disabled={!readyDurations[job.id]?.length} key={job.id} value={job.id}>{job.job_code} — {job.title}</option>)}</select></label><label>Candidate name<input name="candidate_name" required maxLength={120} autoComplete="name" /></label><label>Candidate email<input name="candidate_email" type="email" required maxLength={254} autoComplete="email" /></label><label>Interview duration (minutes)<input key={inviteJob} name="duration_minutes" type="number" min={5} max={180} value={inviteDuration} onChange={event => setInviteDuration(Number(event.target.value))} required /></label><p className="muted">Create a private link to share manually. Email delivery is not connected yet.</p>{!readyDurations[inviteJob]?.includes(inviteDuration) && <p className="configuration-note">No completed set is available for this duration. Choose a duration with a ready set before inviting.</p>}</>}<button className="button" disabled={busy || (modal === 'invite' && !readyDurations[inviteJob]?.includes(inviteDuration))}>{busy ? 'Saving…' : modal === 'job' ? 'Create job' : 'Create invitation'}</button></form>}</section></Dialog>}
    {selected && <Dialog title="detail-title" onClose={() => setSelected(null)}><section className="modal detail-modal"><div className="panel-heading"><div><p className="eyebrow">SESSION DETAILS</p><h2 id="detail-title">{selected.candidate_name}</h2><p>{selected.candidate_email}</p></div><button className="icon-button" aria-label="Close session details" onClick={() => setSelected(null)}>×</button></div><Status state={selected.state} /><h3>{selected.job_title}</h3><p className="job-code">Job ID: <strong>{selected.job_code}</strong></p><div className="detail-grid"><div><small>Duration</small><strong>{selected.duration_minutes} minutes</strong></div><div><small>Invitation expires</small><strong>{date(selected.expires_at)}</strong></div><div><small>Started</small><strong>{date(selected.started_at)}</strong></div><div><small>Submitted</small><strong>{date(selected.submitted_at)}</strong></div></div>{error && <p className="error" role="alert">{error}</p>}<div className="invitation-panel"><h3>Candidate invitation</h3>{invitationIsActive(selected) ? <>{selected.invitation_recoverable === false && <p>This older invitation does not have a saved link. Create a replacement to share; the previous link will stop working.</p>}{detailLink ? <><label>Saved invitation link<textarea readOnly rows={3} value={detailLink} onFocus={e => e.currentTarget.select()} /></label><button className="button" onClick={() => void copyInvitation()}>Copy invitation link</button></> : <><p>Retrieve the private link to share with the candidate again. Its original expiry stays the same.</p><button className="button" disabled={busy || (selected.invitation_recoverable === false && selected.state !== 'invited')} onClick={recoverInvitation}>{busy ? 'Retrieving…' : selected.invitation_recoverable === false ? 'Create replacement link' : 'Show invitation link'}</button>{selected.invitation_recoverable === false && selected.state !== 'invited' && <p>This older session is already accepted. Continue using its original link.</p>}</>}{copyFeedback && <p role="status" className="invitation-feedback">{copyFeedback}</p>}</> : <p>This session has expired or been submitted. Its invitation is no longer available for sharing.</p>}</div><ExpandableSection label={<>Job requirements</>}><p className="preserve">{selected.description}</p></ExpandableSection><QuestionPreparation key={`questions:${selected.id}`} interview={selected} token={token} /><Assessment session={selected.id} token={token} /><RecordingEvidence session={selected.id} token={token} /><ReviewAssistant key={`review:${selected.id}`} session={selected.id} token={token} /><ExpandableSection className="session-timeline" label={<>Session timeline ({events.items.length}) </>} help="Saved audit history for this session. Expand it and use Load older events to browse earlier activity."><Timeline events={events.items} />{events.next_cursor && <button className="load-more" disabled={busy} onClick={() => void run(async () => { const next = await api.audit(selected.id, events.next_cursor!); setEvents({ items: [...events.items, ...next.items], next_cursor: next.next_cursor }); })}>Load older events ↓</button>}</ExpandableSection><p className="muted">Session ID: <code>{selected.id}</code></p></section></Dialog>}
  </div>;
}
