import { JobCard } from './JobCard';
import { modalTypography } from '../../components/ui/modal-typography';
import { ListToolbar } from '../../components/ui/list-toolbar';
import { Input } from '../../components/ui/input';
import { CreateJobForm } from './CreateJobForm';
import { JobFilters } from './JobFilters';
import { CandidatePreparation } from './CandidatePreparation';
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query';
import { HelpTip } from './HelpTip';
import { MetricTrend } from './MetricTrend';
import { Select } from '../../components/ui/select';
import { Pagination } from '../../components/ui/pagination';
import { RefreshButton } from '../../components/ui/refresh-button';
import { AnimatedCount, DashboardCard } from './DashboardCard';
import { Button } from '../../components/ui/button';
import {
  InterviewerProfile,
  useInterviewerAccount,
} from './InterviewerProfile';
import { LiveScreenViewer } from '../live-screen/LiveScreenViewer';
import { auditPresentation } from './auditPresentation';
import { ExpandableSection } from './ExpandableSection';
import {
  ArrowLeft,
  BriefcaseBusiness,
  ChartNoAxesColumn,
  ClipboardList,
  History,
  Home,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  Sun,
  UserRound,
} from 'lucide-react';
import { useWorkspaceTheme } from './useWorkspaceTheme';
import './workspace.css';
import { RecordingEvidence } from './RecordingEvidence';
import { Analytics } from './Analytics';
import { ReviewAssistant } from './ReviewAssistant';
import { Assessment } from './Assessment';
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from 'react';
import { QuestionPreparation } from './QuestionPreparation';
import {
  invitationIsActive,
  workspaceApi,
  type Audit,
  type AuditFilters,
  type Interview,
  type Job,
  type JobListFilters,
  type Page,
} from './api';

const reviewSections = [
  ['overview', 'Job & session'],
  ['questions', 'Questions & rubrics'],
  ['assessment', 'Assessment'],
  ['evidence', 'Evidence & assistant'],
  ['timeline', 'Session timeline'],
] as const;
type ReviewSection = (typeof reviewSections)[number][0];

const JOBS_PER_PAGE = 6;

type View = 'interviewer' | 'reviewer' | 'audit' | 'profile' | 'question-bank';
type WorkspacePage = View | { job: Pick<Job, 'id' | 'title' | 'job_code'> };
const message = (error: unknown) =>
  error instanceof Error
    ? error.message
    : 'Something went wrong. Please retry.';
const date = (value: string | null) =>
  value && Number.isFinite(Date.parse(value))
    ? new Date(value).toLocaleString(undefined, {
        dateStyle: 'medium',
        timeStyle: 'short',
      })
    : '—';
export function Status({ state }: { state: string }) {
  return (
    <span className={`status status-${state}`}>
      {state === 'preparing'
        ? 'Preparing questions'
        : state === 'preparation_failed'
          ? 'Preparation failed'
          : state.replaceAll('_', ' ')}
    </span>
  );
}
function Timeline({
  events,
  emptyMessage = 'No audit events for this session yet.',
}: {
  events: Audit[];
  emptyMessage?: string;
}) {
  if (!events.length)
    return <div className="empty compact">{emptyMessage}</div>;
  return (
    <ol className="timeline">
      {events.map((event) => {
        const { title, performedBy, details } = auditPresentation(event);
        return (
          <li key={event.id}>
            <span className="timeline-dot" />
            <div className="audit-event-content">
              <strong>{title}</strong>
              <p>
                <time dateTime={event.created_at}>
                  {date(event.created_at)}
                </time>{' '}
                · {performedBy}
              </p>
              {(event.job_title || event.job_code) && (
                <p className="audit-context">
                  Job: {event.job_title}
                  {event.job_code && <> · Job ID: {event.job_code}</>}
                </p>
              )}
              {event.candidate_name && (
                <p>
                  Candidate: {event.candidate_name}
                  {event.candidate_email && <> · {event.candidate_email}</>}
                </p>
              )}
              {details.length > 0 && <p>{details.join(' · ')}</p>}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
function AuditTable({ events }: { events: Audit[] }) {
  if (!events.length)
    return (
      <div className="empty compact">No audit events match these filters.</div>
    );
  return (
    <div className="table-scroll">
      <table className="min-w-[44rem]">
        <thead>
          <tr>
            <th scope="col">Event</th>
            <th scope="col">Performed by</th>
            <th scope="col">Job ID</th>
            <th scope="col">Job title</th>
            <th scope="col">Candidate</th>
            <th scope="col">Timestamp</th>
          </tr>
        </thead>
        <tbody>
          {events.map((event) => {
            const { title, performedBy, details } = auditPresentation(event);
            const timestamp = new Date(event.created_at);
            return (
              <tr key={event.id} className="align-top">
                <td>
                  <strong className="font-normal!">{title}</strong>
                  {details.length > 0 && <small>{details.join(' · ')}</small>}
                </td>
                <td>{performedBy}</td>
                <td className="whitespace-nowrap">{event.job_code || '—'}</td>
                <td>{event.job_title || '—'}</td>
                <td>
                  {event.candidate_name || event.candidate_email || '—'}
                  {event.candidate_name && event.candidate_email && (
                    <small>{event.candidate_email}</small>
                  )}
                </td>
                <td className="whitespace-nowrap tabular-nums">
                  <time dateTime={event.created_at}>
                    {timestamp.toLocaleTimeString()}
                    <small>
                      {timestamp.toLocaleDateString(undefined, {
                        dateStyle: 'medium',
                      })}
                    </small>
                  </time>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function Dialog({
  children,
  title,
  onClose,
  busy = false,
}: {
  children: ReactNode;
  title: string;
  onClose: () => void;
  busy?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  return (
    <dialog
      ref={ref}
      className={`modal-backdrop interviewer-workspace ${modalTypography}`}
      aria-labelledby={title}
      onCancel={(event) => {
        event.preventDefault();
        if (!busy) onClose();
      }}
    >
      {children}
    </dialog>
  );
}
const navigationShading = [
  'rounded-r-md rounded-l-none border-0 border-l-2 border-solid border-transparent bg-transparent py-2 text-slate-600',
  'hover:bg-indigo-500/5 hover:text-slate-900 aria-[current]:border-l-indigo-600 aria-[current]:bg-linear-to-r',
  'aria-[current]:from-indigo-500/15 aria-[current]:to-transparent aria-[current]:text-slate-950 workspace-dark:border-transparent workspace-dark:bg-transparent',
  'workspace-dark:text-slate-400 workspace-dark:hover:bg-indigo-400/10 workspace-dark:hover:text-slate-100 workspace-dark:aria-[current]:border-l-indigo-400 workspace-dark:aria-[current]:text-slate-100',
].join(' ');

export function Workspace({
  token,
  onLogout,
}: {
  token: string;
  onLogout: () => void;
}) {
  const account = useInterviewerAccount(token);
  const accountMenu = useRef<HTMLDivElement>(null);
  const { theme, toggleTheme } = useWorkspaceTheme();
  const api = useMemo(() => workspaceApi(token), [token]);
  const createJobMutation = useMutation({ mutationFn: api.createJob });
  const deleteJobMutation = useMutation({ mutationFn: api.deleteJob });
  const inviteMutation = useMutation({
    mutationFn: ({
      job,
      data,
    }: {
      job: string;
      data: Parameters<typeof api.invite>[1];
    }) => api.invite(job, data),
  });
  function inviteCandidate(
    job: string,
    data: Parameters<typeof api.invite>[1],
  ) {
    return inviteMutation.mutateAsync({ job, data });
  }
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [viewHistory, setViewHistory] = useState<WorkspacePage[]>([
    'interviewer',
  ]);
  const page = viewHistory.at(-1) ?? 'interviewer';
  const view = typeof page === 'string' ? page : 'analytics';
  const analyticsJob = typeof page === 'string' ? null : page.job;

  function navigateTo(next: WorkspacePage) {
    setJobCursors([]);
    setViewHistory((history) =>
      history.at(-1) === next ? history : [...history, next],
    );
  }

  function goBack() {
    setJobCursors([]);
    setViewHistory((history) =>
      history.length > 1 ? history.slice(0, -1) : history,
    );
  }
  const [jobFilters, setJobFilters] = useState<JobListFilters>({
    search: '',
    invitations: 'all',
    sort: 'newest',
  });
  const [analyticsFilters, setAnalyticsFilters] = useState<JobListFilters>({
    search: '',
    invitations: 'all',
    sort: 'newest',
  });
  const activeJobFilters =
    view === 'interviewer'
      ? jobFilters
      : view === 'question-bank'
        ? analyticsFilters
        : undefined;
  const hasJobFilters =
    !!activeJobFilters &&
    (!!activeJobFilters.search || activeJobFilters.invitations !== 'all');
  const [jobCursors, setJobCursors] = useState<string[]>([]);
  const [sessionCursors, setSessionCursors] = useState<string[]>([]);
  const [auditCursors, setAuditCursors] = useState<string[]>([]);
  const auditScroll = useRef<HTMLDivElement>(null);
  const [auditForm, setAuditForm] = useState({
    candidate: '',
    job: '',
    from: '',
    until: '',
  });
  const [auditFilters, setAuditFilters] = useState<AuditFilters>({});
  const queryClient = useQueryClient();
  const queryRoot = ['workspace-view', token];
  const jobsQuery = useQuery({
    queryKey: [...queryRoot, 'jobs', jobCursors.at(-1), activeJobFilters],
    placeholderData: keepPreviousData,
    queryFn: ({ signal }) =>
      api.jobs(jobCursors.at(-1), JOBS_PER_PAGE, signal, activeJobFilters),
  });
  const sessionsQuery = useQuery<Page<Interview>>({
    queryKey: [...queryRoot, 'sessions', sessionCursors.at(-1)],
    refetchInterval: (query) =>
      query.state.data?.items.some((session) => session.state === 'preparing')
        ? 5000
        : false,
    placeholderData: keepPreviousData,
    queryFn: ({ signal }) => api.sessions(sessionCursors.at(-1), signal),
  });
  const auditQuery = useQuery({
    queryKey: [...queryRoot, 'audit', auditCursors.at(-1), auditFilters],
    queryFn: ({ signal }) =>
      api.audit(undefined, auditCursors.at(-1), auditFilters, signal),
  });
  const summaryQuery = useQuery({
    queryKey: [...queryRoot, 'summary'],
    queryFn: ({ signal }) => api.summary(signal),
  });
  const jobs = jobsQuery.data ?? { items: [], next_cursor: null };
  const sessions = sessionsQuery.data ?? { items: [], next_cursor: null };
  const audit = auditQuery.data ?? { items: [], next_cursor: null };
  const summary = summaryQuery.data ?? {
    jobs: 0,
    sessions: 0,
    active: 0,
    reviewable: 0,
  };
  const loading =
    jobsQuery.isPending ||
    sessionsQuery.isPending ||
    auditQuery.isPending ||
    summaryQuery.isPending;
  const queryError =
    jobsQuery.error ??
    sessionsQuery.error ??
    auditQuery.error ??
    summaryQuery.error;
  const [selected, setSelected] = useState<Interview | null>(null);
  const [events, setEvents] = useState<Page<Audit>>({
    items: [],
    next_cursor: null,
  });
  const [modal, setModal] = useState<'job' | 'invite' | null>(null);
  const [inviteJob, setInviteJob] = useState('');
  const [inviteJobLocked, setInviteJobLocked] = useState(false);
  const [inviteDuration, setInviteDuration] = useState(10);
  const [expandedJobId, setExpandedJobId] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deletingJob, setDeletingJob] = useState(false);
  const [deleteError, setDeleteError] = useState('');
  const expandedJob = jobs.items.find((job) => job.id === expandedJobId);
  const [pendingInvitation, setPendingInvitation] = useState<string | null>(
    null,
  );
  const [invitation, setInvitation] = useState('');
  const [detailLink, setDetailLink] = useState('');
  const [copyFeedback, setCopyFeedback] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [filter, setFilter] = useState('all');
  const [interviewSort, setInterviewSort] = useState('newest');
  const [auditSort, setAuditSort] = useState('newest');
  const [jobFilter, setJobFilter] = useState('');
  const [search, setSearch] = useState('');
  const [requestKey, setRequestKey] = useState(() => crypto.randomUUID());
  useEffect(() => {
    setError(queryError ? message(queryError) : '');
  }, [queryError]);
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
      setAuditFilters(filters);
      setAuditCursors([]);
      auditScroll.current?.scrollTo(0, 0);
    });
  }
  async function changeAuditPage(direction: 'previous' | 'next') {
    const cursor = audit.next_cursor;
    if (direction === 'next' && !cursor) return;
    const cursors =
      direction === 'next' && cursor
        ? [...auditCursors, cursor]
        : auditCursors.slice(0, -1);
    await run(async () => {
      setAuditCursors(cursors);
      auditScroll.current?.scrollTo(0, 0);
    });
  }

  async function changeJobPage(direction: 'previous' | 'next') {
    const cursor = jobs.next_cursor;
    if (direction === 'next' && !cursor) return;
    const cursors =
      direction === 'next' && cursor
        ? [...jobCursors, cursor]
        : jobCursors.slice(0, -1);
    await run(async () => {
      setJobCursors(cursors);
    });
  }
  const jobPagination = (
    <Pagination
      label="Job pagination"
      page={jobCursors.length + 1}
      summary={`${jobs.total ?? summary.jobs} ${(jobs.total ?? summary.jobs) === 1 ? 'job' : 'jobs'}${hasJobFilters ? ' matching' : ' total'}`}
      hasNext={!!jobs.next_cursor}
      busy={busy || jobsQuery.isFetching}
      onChange={(direction) => void changeJobPage(direction)}
    />
  );
  async function changeSessionPage(direction: 'previous' | 'next') {
    const cursor = sessions.next_cursor;
    if (direction === 'next' && !cursor) return;
    const cursors =
      direction === 'next' && cursor
        ? [...sessionCursors, cursor]
        : sessionCursors.slice(0, -1);
    await run(async () => {
      setSessionCursors(cursors);
    });
  }
  async function deleteJob(job: Job) {
    if (deletingJob) return;
    setDeletingJob(true);
    setDeleteError('');
    try {
      await deleteJobMutation.mutateAsync(job.id);
      setExpandedJobId(null);
      await run(refresh);
    } catch (failure) {
      setDeleteError(message(failure));
    } finally {
      setDeletingJob(false);
    }
  }
  async function refresh() {
    setJobCursors([]);
    setSessionCursors([]);
    setAuditCursors([]);
    await queryClient.invalidateQueries({ queryKey: queryRoot });
    auditScroll.current?.scrollTo(0, 0);
  }
  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError('');
    try {
      await action();
    } catch (error) {
      setError(message(error));
    } finally {
      setBusy(false);
    }
  }
  function openModal(type: 'job' | 'invite', jobId = '') {
    const selected = jobId || jobs.items[0]?.id || '';
    setInviteJob(selected);
    setInviteJobLocked(!!jobId);
    setInviteDuration(
      jobs.items.find((job) => job.id === selected)
        ?.preparation_duration_minutes ?? 10,
    );
    setRequestKey(crypto.randomUUID());
    setPendingInvitation(null);
    setInvitation('');
    setModal(type);
    setError('');
  }
  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    await run(async () => {
      if (modal === 'job') {
        await createJobMutation.mutateAsync({
          question_set_count: 1,
          preparation_duration_minutes: Number(
            form.get('preparation_duration_minutes'),
          ),
          job_code: String(form.get('job_code')),
          title: String(form.get('title')),
          description: String(form.get('description')),
          request_key: requestKey,
        });
        setModal(null);
      } else {
        const result = await inviteCandidate(String(form.get('job_id')), {
          candidate_name: String(form.get('candidate_name')),
          candidate_email: String(form.get('candidate_email')),
          duration_minutes: Number(form.get('duration_minutes')),
          request_key: requestKey,
        });
        setPendingInvitation(result.session.id);
      }
      await refresh();
    });
  }
  const [reviewSection, setReviewSection] = useState<ReviewSection>('overview');

  function inspect(id: string) {
    setReviewSection('overview');
    setDetailLink('');
    setCopyFeedback('');
    void run(async () => {
      const [detail, history] = await Promise.all([
        api.detail(id),
        api.audit(id),
      ]);
      setSelected(detail);
      setEvents(history);
      if (detail.invitation_recoverable && invitationIsActive(detail)) {
        const link = await api.invitation(id);
        setDetailLink(
          `${window.location.origin}${window.location.pathname}#invite=${link.invitation_token}`,
        );
        setEvents(await api.audit(id));
      }
    });
  }
  function recoverInvitation() {
    if (!selected) return;
    void run(async () => {
      const result = await api.invitation(selected.id);
      setDetailLink(
        `${window.location.origin}${window.location.pathname}#invite=${result.invitation_token}`,
      );
      setCopyFeedback(
        result.replaced_legacy_link
          ? 'Replacement created. The previous link no longer works.'
          : 'Saved invitation link ready to share.',
      );
      setSelected({ ...selected, invitation_recoverable: true });
      setEvents(await api.audit(selected.id));
    });
  }
  async function copyInvitation() {
    try {
      await navigator.clipboard.writeText(detailLink);
      setCopyFeedback('Invitation link copied.');
    } catch {
      setCopyFeedback('Select the link above and copy it manually.');
    }
  }
  const interviewJobs = new Set(
    sessions.items.map((session) => session.job_code),
  );
  const visible = sessions.items.filter(
    (s) =>
      (!jobFilter || s.job_code === jobFilter) &&
      (filter === 'all' || s.state === filter) &&
      `${s.candidate_name} ${s.candidate_email} ${s.job_title} ${s.job_code}`
        .toLowerCase()
        .includes(search.toLowerCase()),
  );
  visible.sort((left, right) => {
    if (interviewSort === 'candidate')
      return left.candidate_name.localeCompare(right.candidate_name);
    if (interviewSort === 'job')
      return left.job_code.localeCompare(right.job_code, undefined, {
        numeric: true,
      });
    const difference =
      Date.parse(left.created_at) - Date.parse(right.created_at);
    return interviewSort === 'oldest' ? difference : -difference;
  });
  const auditEvents =
    auditSort === 'oldest' ? [...audit.items].reverse() : audit.items;
  return (
    <div
      className="interviewer-workspace flex min-h-screen [--sidebar-width:clamp(248px,18vw,350px)] bg-slate-50 text-slate-700 max-sm:flex-col workspace-dark:bg-slate-950 workspace-dark:text-slate-100"
      data-theme={theme}
    >
      <aside
        id="interviewer-sidebar"
        className={`flex flex-col overflow-hidden max-sm:border-b max-sm:border-solid max-sm:p-3 border-indigo-200 bg-indigo-100 sm:fixed sm:inset-y-0 sm:left-0 sm:z-10 sm:overflow-y-auto sm:px-[var(--nav-inset)] sm:after:pointer-events-none sm:after:absolute sm:after:bottom-0 sm:after:top-16 sm:after:right-0 sm:after:w-px sm:after:bg-indigo-200 sm:after:content-[""] workspace-dark:border-slate-800 workspace-dark:bg-slate-900 workspace-dark:sm:after:bg-slate-800 motion-safe:transition-[width,padding] motion-safe:duration-200 motion-safe:ease-out ${
          sidebarOpen
            ? 'sm:w-[var(--sidebar-width)] sm:[--nav-inset:1rem] 2xl:[--nav-inset:1.25rem]'
            : 'sm:w-18 sm:[--nav-inset:0.75rem]'
        }`}
      >
        <div
          className={`flex items-center gap-2 border-b border-indigo-200 pb-3 sm:-mx-[var(--nav-inset)] sm:px-[var(--nav-inset)] sm:border-slate-200 sm:h-16 sm:shrink-0 sm:pb-0 workspace-dark:border-slate-800 ${sidebarOpen ? 'justify-between' : 'justify-center'}`}
        >
          {sidebarOpen && (
            <a
              className="flex min-w-0 items-center gap-2 text-subheading font-semibold tracking-tight"
              href="#"
              onClick={(event) => {
                event.preventDefault();
                navigateTo('interviewer');
              }}
            >
              <span aria-hidden="true" className="logo-mark shrink-0">
                i
              </span>
              <span className="font-display text-subheading font-semibold tracking-tight uppercase text-primary workspace-dark:text-indigo-400">
                Interview
              </span>
              <span className="text-indigo-500"> / </span>
            </a>
          )}
          <Button
            aria-label={
              sidebarOpen ? 'Collapse navigation' : 'Expand navigation'
            }
            aria-controls="interview-navigation"
            aria-expanded={sidebarOpen}
            className="size-[34px] min-h-[34px] shrink-0 p-0"
            onClick={() => setSidebarOpen((open) => !open)}
          >
            {sidebarOpen ? (
              <PanelLeftClose size={18} aria-hidden="true" />
            ) : (
              <PanelLeftOpen size={18} aria-hidden="true" />
            )}
          </Button>
        </div>
        <nav
          id="interview-navigation"
          className="mt-5 flex min-h-0 flex-1 flex-col gap-2 max-sm:mt-3 max-sm:grid max-sm:grid-cols-4"
          aria-label="Interview navigation"
        >
          {(
            [
              ['interviewer', 'Jobs & invitations', 'Jobs', BriefcaseBusiness],
              ['reviewer', 'Interview results', 'Results', ClipboardList],
              [
                'question-bank',
                'Question-bank analytics',
                'Analytics',
                ChartNoAxesColumn,
              ],
              ['audit', 'Audit trail', 'Audit', History],
            ] as const
          ).map(([key, label, shortLabel, Icon]) => (
            <Button
              key={key}
              aria-label={label}
              title={sidebarOpen ? undefined : label}
              aria-current={
                view === key ||
                (view === 'analytics' && key === 'question-bank')
                  ? 'page'
                  : undefined
              }
              className={`min-h-11 w-full min-w-0 ${navigationShading} text-control font-normal ${sidebarOpen ? 'px-2 sm:justify-start sm:px-3' : 'justify-center px-0'}`}
              onClick={() => navigateTo(key)}
            >
              <span
                className="inline-grid size-[23px] shrink-0 place-items-center"
                aria-hidden="true"
              >
                <Icon size={16} />
              </span>
              {sidebarOpen && (
                <>
                  <span className="hidden truncate sm:inline">{label}</span>
                  <span className="truncate sm:hidden">{shortLabel}</span>
                </>
              )}
            </Button>
          ))}
        </nav>
      </aside>
      <div
        className={`flex min-w-0 flex-1 flex-col motion-safe:transition-[margin-left] motion-safe:duration-200 motion-safe:ease-out ${sidebarOpen ? 'sm:ml-[var(--sidebar-width)]' : 'sm:ml-18'}`}
      >
        <header className="sticky top-0 z-10 flex h-16 shrink-0 items-center justify-between gap-2 border-b border-solid border-slate-200 bg-white px-4 text-control text-slate-700 sm:px-6 workspace-dark:border-slate-800 workspace-dark:bg-slate-900 workspace-dark:text-slate-200">
          <div className="flex min-w-0 items-center gap-3">
            <nav
              aria-label="Workspace navigation"
              className="flex shrink-0 gap-1"
            >
              <Button
                type="button"
                aria-label="Go back"
                title="Back"
                disabled={viewHistory.length < 2}
                className="size-[34px] min-h-[34px] p-0"
                onClick={goBack}
              >
                <ArrowLeft
                  size={18}
                  className="text-violet-700 workspace-dark:text-violet-300"
                  aria-hidden="true"
                />
              </Button>
              <Button
                type="button"
                aria-label="Go home"
                title="Home"
                className="size-[34px] min-h-[34px] p-0"
                onClick={() => navigateTo('interviewer')}
              >
                <Home size={18} aria-hidden="true" />
              </Button>
            </nav>
            <span className="hidden truncate sm:inline">
              Interviews{' '}
              <span className="muted">
                /{' '}
                {view === 'analytics' || view === 'question-bank'
                  ? 'Question-bank analytics'
                  : view === 'profile'
                    ? 'Profile'
                    : view === 'audit'
                      ? 'Audit trail'
                      : view === 'reviewer'
                        ? 'Interview results'
                        : 'Jobs & invitations'}
              </span>
            </span>
          </div>
          <div className="flex shrink-0 items-center gap-2 sm:gap-4">
            <button
              type="button"
              role="switch"
              aria-label="Dark mode"
              aria-checked={theme === 'dark'}
              onClick={toggleTheme}
              className="group inline-flex min-h-10 items-center gap-2 rounded-lg border-0 bg-transparent px-[7.2px] text-slate-600 focus-visible:outline-2 focus-visible:outline-offset-2 workspace-dark:text-slate-300"
            >
              <Sun size={16} aria-hidden="true" />
              <span
                aria-hidden="true"
                className="relative h-5 w-9 rounded-full bg-slate-400 transition-colors group-aria-checked:bg-primary motion-reduce:transition-none"
              >
                <span className="absolute top-0.5 left-0.5 size-4 rounded-full bg-white shadow-sm transition-transform group-aria-checked:translate-x-4 motion-reduce:transition-none" />
              </span>
              <Moon size={16} aria-hidden="true" />
            </button>
            <Button
              className="size-[34px] min-h-0 rounded-full p-0"
              aria-label="Account details"
              popoverTarget="interviewer-account"
            >
              <UserRound size={17} aria-hidden="true" />
            </Button>
            <div
              ref={accountMenu}
              id="interviewer-account"
              popover="auto"
              aria-label="Account details"
              className="fixed inset-auto top-16 right-4 m-0 w-72 max-w-[calc(100vw-2rem)] rounded-xl border border-solid border-slate-200 bg-white p-4 text-slate-700 shadow-lg workspace-dark:border-slate-700 workspace-dark:bg-slate-900 workspace-dark:text-slate-100"
            >
              <p className="m-0 text-detail font-semibold uppercase tracking-wide">
                Interviewer account
              </p>
              <p
                className="mt-2 mb-4 break-all text-control"
                aria-live="polite"
              >
                {account.email ||
                  (account.error ? 'Email unavailable' : 'Loading email…')}
              </p>
              <div className="flex flex-wrap gap-2">
                <Button
                  onClick={() => {
                    accountMenu.current?.hidePopover();
                    navigateTo('profile');
                  }}
                >
                  View profile
                </Button>
                <Button
                  onClick={() => {
                    accountMenu.current?.hidePopover();
                    onLogout();
                  }}
                >
                  Sign out
                </Button>
              </div>
            </div>
          </div>
        </header>
        <main className="dashboard flex w-full flex-1 flex-col">
          {view === 'profile' ? (
            <InterviewerProfile account={account} onLogout={onLogout} />
          ) : analyticsJob ? (
            <section aria-labelledby="analytics-title">
              <header className="page-heading">
                <div className="min-w-0">
                  <h1 id="analytics-title" className="font-display">
                    Question-bank analytics
                  </h1>
                  <p className="mb-0 break-words text-control">
                    {analyticsJob.title} · {analyticsJob.job_code}
                  </p>
                </div>
              </header>
              <Analytics
                key={analyticsJob.id}
                job={analyticsJob.id}
                token={token}
              />
            </section>
          ) : (
            <>
              <div className="page-heading">
                <div>
                  <h1 className="font-display">
                    {view === 'question-bank'
                      ? 'Question-bank analytics'
                      : view === 'reviewer'
                        ? 'Interview results'
                        : view === 'audit'
                          ? 'Audit trail'
                          : 'Jobs & invitations'}
                  </h1>
                </div>
                <div className="actions items-center">
                  <RefreshButton
                    pending={refreshing}
                    disabled={busy || loading}
                    onClick={() => {
                      setRefreshing(true);
                      setError('');
                      void refresh()
                        .catch((failure) => setError(message(failure)))
                        .finally(() => setRefreshing(false));
                    }}
                  >
                    Refresh
                  </RefreshButton>
                  {view === 'interviewer' && (
                    <Button
                      variant="standard"
                      compact
                      disabled={busy || loading}
                      onClick={() => openModal('job')}
                    >
                      + Create job
                    </Button>
                  )}
                </div>
              </div>
              {error && (
                <div className="error" role="alert">
                  {error}
                </div>
              )}
              {view === 'reviewer' && (
                <div className="stats">
                  {(
                    [
                      [
                        'Total roles',
                        summary.jobs,
                        'Job descriptions created',
                        summary.daily_roles,
                        'Roles created',
                      ],
                      [
                        'Invitations',
                        summary.sessions,
                        'Across all your roles',
                        summary.daily_invitations,
                        'Invitations sent',
                      ],
                      [
                        'Active sessions',
                        summary.active,
                        'Ready or in progress. The trend shows interviews started each day.',
                        summary.daily_starts,
                        'Interviews started',
                      ],
                      [
                        'Ready for review',
                        summary.reviewable,
                        'Submitted, scoring, or completed. The trend shows interviews submitted each day.',
                        summary.daily_submissions,
                        'Interviews submitted',
                      ],
                    ] as const
                  ).map(([label, value, help, counts, trendLabel], index) => (
                    <DashboardCard variant="metric" index={index} key={label}>
                      <span className="flex items-center justify-between gap-2">
                        {label}
                        <HelpTip text={help} />
                      </span>
                      <AnimatedCount value={loading ? null : value} />
                      <MetricTrend
                        counts={counts}
                        endDate={summary.activity_end_date}
                        label={trendLabel}
                      />
                    </DashboardCard>
                  ))}
                </div>
              )}
              {loading ? (
                <section className="panel empty" role="status">
                  Loading your interviews…
                </section>
              ) : view === 'audit' ? (
                <section className="panel" aria-label="Audit events">
                  <form
                    className="block! px-5 py-4"
                    onSubmit={applyAuditFilters}
                    onReset={() => {
                      setAuditForm({
                        candidate: '',
                        job: '',
                        from: '',
                        until: '',
                      });
                      void loadAudit({});
                    }}
                  >
                    <ListToolbar
                      search={{
                        name: 'candidate',
                        value: auditForm.candidate,
                        placeholder: 'Search candidates…',
                        'aria-label': 'Search audit events by candidate',
                        onChange: (event) =>
                          setAuditForm({
                            ...auditForm,
                            candidate: event.target.value,
                          }),
                      }}
                      filtersActive={
                        !!(
                          auditFilters.job ||
                          auditFilters.from ||
                          auditFilters.until
                        )
                      }
                      sort={
                        <label>
                          Sort this page
                          <Select
                            value={auditSort}
                            onChange={(event) =>
                              setAuditSort(event.target.value)
                            }
                          >
                            <option value="newest">Newest first</option>
                            <option value="oldest">Oldest first</option>
                          </Select>
                        </label>
                      }
                      filters={
                        <>
                          <label>
                            Job
                            <Input
                              name="job"
                              value={auditForm.job}
                              onChange={(event) =>
                                setAuditForm({
                                  ...auditForm,
                                  job: event.target.value,
                                })
                              }
                              placeholder="Title or job ID"
                            />
                          </label>
                          <div className="grid grid-cols-2 gap-2">
                            <label>
                              From date
                              <Input
                                type="date"
                                name="from"
                                title="From date"
                                value={auditForm.from}
                                onChange={(event) =>
                                  setAuditForm({
                                    ...auditForm,
                                    from: event.target.value,
                                  })
                                }
                              />
                            </label>
                            <label>
                              To date
                              <Input
                                type="date"
                                name="until"
                                title="To date"
                                value={auditForm.until}
                                onChange={(event) =>
                                  setAuditForm({
                                    ...auditForm,
                                    until: event.target.value,
                                  })
                                }
                              />
                            </label>
                          </div>
                          <div className="flex justify-end gap-2">
                            <Button compact type="reset" disabled={busy}>
                              Clear
                            </Button>
                            <Button compact type="submit" disabled={busy}>
                              Apply
                            </Button>
                          </div>
                        </>
                      }
                    />
                  </form>
                  <div
                    ref={auditScroll}
                    className="audit-scroll"
                    role="region"
                    aria-label="Audit events"
                    tabIndex={0}
                  >
                    {audit.items.length === 0 &&
                    (auditFilters.from || auditFilters.until) ? (
                      <div className="empty compact">
                        <p>
                          No audit events match the selected dates and search
                          filters.
                        </p>
                        <Button
                          disabled={busy}
                          onClick={() => {
                            setAuditForm((form) => ({
                              ...form,
                              from: '',
                              until: '',
                            }));
                            const filters: AuditFilters = {};
                            if (auditFilters.candidate)
                              filters.candidate = auditFilters.candidate;
                            if (auditFilters.job)
                              filters.job = auditFilters.job;
                            void loadAudit(filters);
                          }}
                        >
                          Search all dates
                        </Button>
                      </div>
                    ) : (
                      <AuditTable events={auditEvents} />
                    )}
                  </div>
                  <Pagination
                    label="Audit pagination"
                    page={auditCursors.length + 1}
                    summary={`${auditEvents.length} on this page${audit.total !== undefined ? ` · ${audit.total} matching ${audit.total === 1 ? 'event' : 'events'}` : ''}`}
                    hasNext={!!audit.next_cursor}
                    busy={busy || auditQuery.isFetching}
                    onChange={(direction) => void changeAuditPage(direction)}
                  />
                </section>
              ) : (
                <>
                  {view === 'interviewer' && (
                    <section
                      className="panel"
                      aria-label="Your roles"
                      aria-busy={jobsQuery.isFetching}
                    >
                      <JobFilters
                        value={jobFilters}
                        onApply={(filters) => {
                          setJobCursors([]);
                          setJobFilters(filters);
                        }}
                      />
                      {jobsQuery.isError ? (
                        <div
                          className="grid justify-items-center gap-3 px-6 py-10"
                          role="alert"
                        >
                          <p>Jobs couldn’t be loaded.</p>
                          <Button
                            compact
                            onClick={() => void jobsQuery.refetch()}
                          >
                            Retry loading
                          </Button>
                        </div>
                      ) : jobs.items.length ? (
                        <div className="job-grid items-stretch! pt-6">
                          {jobs.items.map((job, index) => (
                            <JobCard key={job.id} job={job} index={index} onOpen={() => {
                              setConfirmDelete(false);
                              setDeleteError('');
                              setExpandedJobId(job.id);
                            }} />
                          ))}
                        </div>
                      ) : (
                        <div className="empty">
                          <div className="empty-icon">▧</div>
                          <h3>
                            {hasJobFilters ? 'No matching jobs' : 'No jobs yet'}
                          </h3>
                          <Button
                            variant="standard"
                            onClick={() => {
                              if (hasJobFilters) {
                                setJobCursors([]);
                                setJobFilters({
                                  search: '',
                                  invitations: 'all',
                                  sort: 'newest',
                                });
                              } else openModal('job');
                            }}
                          >
                            {hasJobFilters ? 'Clear filters' : '+ Create job'}
                          </Button>
                        </div>
                      )}
                      {jobPagination}
                    </section>
                  )}
                  {view === 'reviewer' && (
                    <section
                      className="panel"
                      aria-label="Candidate interviews"
                    >
                      <ListToolbar
                        className="mx-5 my-4"
                        search={{
                          'aria-label': 'Search interviews on this page',
                          placeholder: 'Search this page…',
                          value: search,
                          onChange: (event) => setSearch(event.target.value),
                        }}
                        filtersActive={!!jobFilter || filter !== 'all'}
                        sort={
                          <label>
                            Sort this page
                            <Select
                              aria-label="Sort interviews on this page"
                              value={interviewSort}
                              onChange={(event) =>
                                setInterviewSort(event.target.value)
                              }
                            >
                              <option value="newest">Newest invited</option>
                              <option value="oldest">Oldest invited</option>
                              <option value="candidate">Candidate A–Z</option>
                              <option value="job">Job ID</option>
                            </Select>
                          </label>
                        }
                        filters={
                          <>
                            <label>
                              Job
                              <Select
                                aria-label="Filter interviews by job ID on this page"
                                value={jobFilter}
                                onChange={(event) =>
                                  setJobFilter(event.target.value)
                                }
                              >
                                <option value="">All jobs</option>
                                {jobFilter && !interviewJobs.has(jobFilter) && (
                                  <option value={jobFilter}>{jobFilter}</option>
                                )}
                                {[...interviewJobs].sort().map((code) => (
                                  <option key={code} value={code}>
                                    {code}
                                  </option>
                                ))}
                              </Select>
                            </label>
                            <label>
                              Status
                              <Select
                                aria-label="Filter interview status on this page"
                                value={filter}
                                onChange={(event) =>
                                  setFilter(event.target.value)
                                }
                              >
                                <option value="all">All statuses</option>
                                {[
                                  'preparing',
                                  'preparation_failed',
                                  'invited',
                                  'ready',
                                  'in_progress',
                                  'submitted',
                                  'scoring',
                                  'completed',
                                  'expired',
                                ].map((status) => (
                                  <option key={status} value={status}>
                                    {status.replaceAll('_', ' ')}
                                  </option>
                                ))}
                              </Select>
                            </label>
                            <Button
                              compact
                              onClick={() => {
                                setJobFilter('');
                                setFilter('all');
                              }}
                            >
                              Clear filters
                            </Button>
                          </>
                        }
                      />
                      {visible.length ? (
                        <div className="table-scroll">
                          <table>
                            <thead>
                              <tr>
                                <th>Candidate</th>
                                <th>Role</th>
                                <th>Status</th>
                                <th>Duration</th>
                                <th>Invited</th>
                                <th>
                                  <span className="sr-only">Actions</span>
                                </th>
                              </tr>
                            </thead>
                            <tbody>
                              {visible.map((s) => (
                                <tr key={s.id}>
                                  <td>
                                    <strong>{s.candidate_name}</strong>
                                    <small>{s.candidate_email}</small>
                                  </td>
                                  <td>
                                    {s.job_title}
                                    <small>Job ID: {s.job_code}</small>
                                  </td>
                                  <td>
                                    <Status state={s.state} />
                                  </td>
                                  <td>{s.duration_minutes} min</td>
                                  <td>{date(s.created_at)}</td>
                                  <td>
                                    <Button
                                      variant="text"
                                      className="accent"
                                      disabled={busy}
                                      onClick={() => inspect(s.id)}
                                    >
                                      View →
                                    </Button>
                                  </td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </div>
                      ) : (
                        <div className="empty compact">
                          <h3>
                            {sessions.items.length
                              ? 'No matching sessions on this page'
                              : 'No candidates yet'}
                          </h3>
                          <p>
                            {sessions.items.length
                              ? 'Try another search or status.'
                              : 'Invite a candidate to prepare their questions. Progress and results will appear here.'}
                          </p>
                        </div>
                      )}
                      <Pagination
                        label="Interview pagination"
                        page={sessionCursors.length + 1}
                        summary={`${summary.sessions} ${summary.sessions === 1 ? 'interview' : 'interviews'} total`}
                        hasNext={!!sessions.next_cursor}
                        busy={busy || sessionsQuery.isFetching}
                        onChange={(direction) =>
                          void changeSessionPage(direction)
                        }
                      />
                    </section>
                  )}
                  {view === 'question-bank' && (
                    <section className="panel" aria-label="Jobs for analytics">
                      <JobFilters
                        value={analyticsFilters}
                        onApply={(filters) => {
                          setJobCursors([]);
                          setAnalyticsFilters(filters);
                        }}
                      />
                      {jobs.items.length === 0 && (
                        <p className="empty">
                          {hasJobFilters
                            ? 'No jobs match these filters.'
                            : 'Create a job to see question-bank analytics here.'}
                        </p>
                      )}
                      <div className="job-grid items-stretch! pt-6">
                        {jobs.items.map((job, index) => (
                          <JobCard key={job.id} job={job} index={index} analytics onOpen={() => {
                            navigateTo({ job });
                            window.scrollTo(0, 0);
                          }} />
                        ))}
                      </div>
                      {jobPagination}
                    </section>
                  )}
                </>
              )}
            </>
          )}
          <footer className="mt-auto">
            INTERVIEW PLATFORM{' '}
            <span>Structured interviews · Evidence-grounded review</span>
          </footer>
        </main>
      </div>
      {expandedJob && (
        <Dialog
          title="job-details-title"
          busy={deletingJob}
          onClose={() => setExpandedJobId(null)}
        >
          <section className="modal detail-modal">
            <div className="panel-heading">
              <div className="min-w-0">
                <p className="eyebrow">JOB DETAILS</p>
                <h2 id="job-details-title" className="break-words">
                  {expandedJob.title}
                </h2>
                <p>
                  Job ID {expandedJob.job_code} · {expandedJob.session_count}{' '}
                  {expandedJob.session_count === 1
                    ? 'invitation'
                    : 'invitations'}
                </p>
              </div>
              <Button
                type="button"
                className="icon-button shrink-0"
                aria-label="Close job details"
                disabled={deletingJob}
                onClick={() => setExpandedJobId(null)}
              >
                ×
              </Button>
            </div>
            <div className="my-6">
              <h3>Job description</h3>
              <p className="preserve max-w-prose break-words">
                {expandedJob.description}
              </p>
            </div>
            <div className="mt-6 flex items-start justify-between gap-3">
              <div className="flex min-w-0 flex-wrap items-center gap-3">
                <Button
                  onClick={() => {
                    setExpandedJobId(null);
                    openModal('invite', expandedJob.id);
                  }}
                >
                  Invite a candidate
                </Button>
              </div>
              <Button
                className="shrink-0 text-rose-700 workspace-dark:text-rose-300"
                aria-haspopup="dialog"
                onClick={() => {
                  setDeleteError('');
                  setConfirmDelete(true);
                }}
              >
                Delete job
              </Button>
            </div>
          </section>
        </Dialog>
      )}
      {expandedJob && confirmDelete && (
        <Dialog
          title="delete-job-title"
          busy={deletingJob}
          onClose={() => setConfirmDelete(false)}
        >
          <section className="modal w-[min(28rem,calc(100vw-2rem))]!">
            <h2 id="delete-job-title">
              Permanently delete {expandedJob.job_code}?
            </h2>
            <p>
              This deletes the job, question sets, invitations, interviews,
              answers, scores, and related audit and proctoring evidence. This
              action cannot be undone.
            </p>
            {deleteError && (
              <p role="alert" className="error">
                {deleteError}
              </p>
            )}
            <div className="mt-4 flex flex-wrap justify-end gap-2">
              <Button
                autoFocus
                disabled={deletingJob}
                onClick={() => setConfirmDelete(false)}
              >
                Cancel
              </Button>
              <Button
                disabled={deletingJob}
                className="border-rose-300 bg-rose-100 text-rose-950 hover:bg-rose-200 workspace-dark:border-rose-800 workspace-dark:bg-rose-900 workspace-dark:text-rose-100"
                onClick={() => void deleteJob(expandedJob)}
              >
                {deletingJob ? 'Deleting…' : 'Delete permanently'}
              </Button>
            </div>
          </section>
        </Dialog>
      )}
      {modal && (
        <Dialog title="modal-title" onClose={() => setModal(null)} busy={busy}>
          <section
            className={
              modal === 'job'
                ? 'modal flex! max-h-[90dvh]! w-[min(640px,calc(100vw-2rem))]! flex-col! overflow-hidden! p-0!'
                : 'modal'
            }
          >
            <div
              className={
                modal === 'job'
                  ? 'panel-heading m-0! shrink-0 border-b border-slate-200 px-6! py-4! workspace-dark:border-slate-700'
                  : 'panel-heading'
              }
            >
              <h2 id="modal-title">
                {modal === 'job'
                  ? 'Create a job'
                  : pendingInvitation
                    ? 'Candidate preparation'
                    : invitation
                      ? 'Invitation created'
                      : 'Invite a candidate'}
              </h2>
              <button
                className="icon-button"
                aria-label="Close dialog"
                disabled={busy}
                onClick={() => setModal(null)}
              >
                ×
              </button>
            </div>
            {error && (
              <p className="error" role="alert">
                {error}
              </p>
            )}
            {modal === 'job' ? (
              <CreateJobForm
                onSubmit={save}
                onChange={() => setRequestKey(crypto.randomUUID())}
                onCancel={() => setModal(null)}
                busy={busy}
              />
            ) : pendingInvitation ? (
              <CandidatePreparation token={token} session={pendingInvitation} />
            ) : invitation ? (
              <>
                <p>
                  Share this private link with the candidate. The link grants
                  access to this session and expires in seven days.
                </p>
                <label>
                  Candidate invitation link
                  <textarea
                    readOnly
                    rows={4}
                    value={invitation}
                    onFocus={(e) => e.currentTarget.select()}
                  />
                </label>
                <p className="muted">
                  This link is saved securely. You can retrieve it again from
                  the session details to share later.
                </p>
                <Button variant="standard" onClick={() => setModal(null)}>
                  Done
                </Button>
              </>
            ) : (
              <form
                onSubmit={save}
                onChange={() => setRequestKey(crypto.randomUUID())}
              >
                <>
                  {inviteJobLocked ? (
                    <div className="grid gap-1 text-control">
                      <span className="text-slate-500 workspace-dark:text-slate-400">Job</span>
                      <span>{jobs.items.find((job) => job.id === inviteJob)?.job_code} — {jobs.items.find((job) => job.id === inviteJob)?.title}</span>
                      <input type="hidden" name="job_id" value={inviteJob} />
                    </div>
                  ) : (
                  <label>
                    Role
                    <Select
                      name="job_id"
                      defaultValue={inviteJob || jobs.items[0]?.id}
                      onChange={(event) => {
                        setInviteJob(event.target.value);
                        setInviteDuration(
                          jobs.items.find(
                            (job) => job.id === event.target.value,
                          )?.preparation_duration_minutes ?? 10,
                        );
                      }}
                      required
                    >
                      {jobs.items.map((job) => (
                        <option key={job.id} value={job.id}>
                          {job.job_code} — {job.title}
                        </option>
                      ))}
                    </Select>
                  </label>
                  )}
                  <label>
                    Candidate name
                    <input
                      name="candidate_name"
                      required
                      maxLength={120}
                      autoComplete="name"
                    />
                  </label>
                  <label>
                    Candidate email
                    <input
                      name="candidate_email"
                      type="email"
                      required
                      maxLength={254}
                      autoComplete="email"
                    />
                  </label>
                  <div className="flex items-center justify-between gap-3 text-control">
                    <span className="text-slate-500 workspace-dark:text-slate-400">Duration</span>
                    <span>{inviteDuration} minutes</span>
                    <input type="hidden" name="duration_minutes" value={inviteDuration} />
                  </div>
                  <p className="muted">
                    Prepare fresh questions for this candidate. The private link
                    becomes available when preparation completes. Email delivery
                    is not connected yet.
                  </p>
                </>
                <Button
                  type="submit"
                  variant="standard"
                  disabled={busy || refreshing}
                >
                  {busy ? 'Saving…' : 'Prepare invitation'}
                </Button>
              </form>
            )}
          </section>
        </Dialog>
      )}
      {selected && (
        <Dialog title="detail-title" onClose={() => setSelected(null)}>
          <section className="modal detail-modal flex! h-[88dvh] w-[min(1280px,calc(100vw-2rem))]! flex-col overflow-hidden! p-0!">
            <div className="flex shrink-0 items-center justify-between gap-4 border-b border-slate-200 px-5 py-4 sm:px-6 workspace-dark:border-slate-700">
              <div className="min-w-0">
                <p className="eyebrow m-0!">SESSION DETAILS</p>
                <h2
                  id="detail-title"
                  className="mt-1 truncate text-section-title font-semibold"
                >
                  {selected.candidate_name}
                </h2>
                <p className="m-0! truncate text-control text-slate-500 workspace-dark:text-slate-400">
                  {selected.candidate_email}
                </p>
              </div>
              <button
                className="icon-button shrink-0"
                aria-label="Close session details"
                onClick={() => setSelected(null)}
              >
                ×
              </button>
            </div>
            <div className="grid min-h-0 flex-1 grid-rows-[auto_minmax(0,1fr)] md:grid-cols-[13rem_minmax(0,1fr)] md:grid-rows-[minmax(0,1fr)]">
              <nav
                aria-label="Session sections"
                className="flex min-w-0 gap-1 overflow-x-auto border-b border-slate-200 bg-slate-50 p-3 md:flex-col md:border-r md:border-b-0 workspace-dark:border-slate-700 workspace-dark:bg-slate-900"
              >
                {reviewSections.map(([id, label]) => (
                  <button
                    key={id}
                    type="button"
                    aria-current={reviewSection === id ? 'true' : undefined}
                    onClick={() => {
                      setReviewSection(id);
                      document
                        .getElementById('session-review-content')
                        ?.scrollTo(0, 0);
                    }}
                    className={`shrink-0 whitespace-nowrap px-3 text-left text-control font-medium ${navigationShading}`}
                  >
                    {label}
                  </button>
                ))}
              </nav>
              <div
                id="session-review-content"
                role="region"
                aria-label="Selected session section"
                tabIndex={0}
                className="min-h-0 min-w-0 overflow-y-auto overscroll-contain [scrollbar-gutter:stable] p-4 sm:p-6 [&>section>.question-preparation:first-child]:mt-0!"
              >
                <section
                  hidden={reviewSection !== 'overview'}
                  aria-label="Job and session"
                >
                  <Status state={selected.state} />
                  <h3>{selected.job_title}</h3>
                  <p className="job-code">
                    Job ID: <strong>{selected.job_code}</strong>
                  </p>
                  <div className="detail-grid">
                    <div>
                      <small>Duration</small>
                      <strong>{selected.duration_minutes} minutes</strong>
                    </div>
                    <div>
                      <small>Invitation expires</small>
                      <strong>{date(selected.expires_at)}</strong>
                    </div>
                    <div>
                      <small>Started</small>
                      <strong>{date(selected.started_at)}</strong>
                    </div>
                    <div>
                      <small>Submitted</small>
                      <strong>{date(selected.submitted_at)}</strong>
                    </div>
                  </div>
                  {error && (
                    <p className="error" role="alert">
                      {error}
                    </p>
                  )}
                  {['preparing', 'preparation_failed'].includes(
                    selected.state,
                  ) && (
                    <CandidatePreparation token={token} session={selected.id} />
                  )}
                  {invitationIsActive(selected) && (
                    <div className="invitation-panel">
                      <h3>Candidate invitation</h3>
                      {selected.invitation_recoverable === false && (
                        <p>
                          This older invitation does not have a saved link.
                          Create a replacement to share; the previous link will
                          stop working.
                        </p>
                      )}
                      {detailLink ? (
                        <>
                          <label>
                            Saved invitation link
                            <textarea
                              readOnly
                              rows={3}
                              value={detailLink}
                              onFocus={(e) => e.currentTarget.select()}
                            />
                          </label>
                          <Button
                            variant="standard"
                            onClick={() => void copyInvitation()}
                          >
                            Copy invitation link
                          </Button>
                        </>
                      ) : (
                        <>
                          <p>
                            Retrieve the private link to share with the
                            candidate again. Its original expiry stays the same.
                          </p>
                          <Button
                            variant="standard"
                            disabled={
                              busy ||
                              (selected.invitation_recoverable === false &&
                                selected.state !== 'invited')
                            }
                            onClick={recoverInvitation}
                          >
                            {busy
                              ? 'Retrieving…'
                              : selected.invitation_recoverable === false
                                ? 'Create replacement link'
                                : 'Show invitation link'}
                          </Button>
                          {selected.invitation_recoverable === false &&
                            selected.state !== 'invited' && (
                              <p>
                                This older session is already accepted. Continue
                                using its original link.
                              </p>
                            )}
                        </>
                      )}
                      {copyFeedback && (
                        <p role="status" className="invitation-feedback">
                          {copyFeedback}
                        </p>
                      )}
                    </div>
                  )}
                  <ExpandableSection label={<>Job requirements</>}>
                    <p className="preserve">{selected.description}</p>
                  </ExpandableSection>
                </section>
                <section
                  hidden={reviewSection !== 'questions'}
                  aria-label="Questions and rubrics"
                >
                  <QuestionPreparation
                    key={`questions:${selected.id}`}
                    interview={selected}
                    token={token}
                  />
                </section>
                <section
                  hidden={reviewSection !== 'assessment'}
                  aria-label="Assessment"
                >
                  <Assessment session={selected.id} token={token} />
                </section>
                <section
                  hidden={reviewSection !== 'evidence'}
                  aria-label="Evidence and assistant"
                >
                  {selected.state === 'in_progress' && (
                    <LiveScreenViewer
                      key={`live:${selected.id}`}
                      session={selected.id}
                      token={token}
                    />
                  )}
                  <RecordingEvidence session={selected.id} token={token} />
                  <ReviewAssistant
                    key={`review:${selected.id}`}
                    session={selected.id}
                    token={token}
                  />
                </section>
                <section
                  hidden={reviewSection !== 'timeline'}
                  aria-label="Session timeline"
                >
                  <ExpandableSection
                    className="session-timeline"
                    label={<>Session timeline ({events.items.length}) </>}
                    help="Saved audit history for this session. Expand it and use Load older events to browse earlier activity."
                  >
                    <Timeline events={events.items} />
                    {events.next_cursor && (
                      <button
                        className="load-more"
                        disabled={busy}
                        onClick={() =>
                          void run(async () => {
                            const next = await api.audit(
                              selected.id,
                              events.next_cursor!,
                            );
                            setEvents({
                              items: [...events.items, ...next.items],
                              next_cursor: next.next_cursor,
                            });
                          })
                        }
                      >
                        Load older events ↓
                      </button>
                    )}
                  </ExpandableSection>
                  <p className="muted">
                    Session ID: <code>{selected.id}</code>
                  </p>
                </section>
              </div>
            </div>
          </section>
        </Dialog>
      )}
    </div>
  );
}
