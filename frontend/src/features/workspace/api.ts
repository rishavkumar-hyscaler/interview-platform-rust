import { request } from '../../api';
export type Page<T> = { items: T[]; next_cursor: string | null };
export type Job = { id: string; job_code: string; title: string; description: string; created_at: string; session_count: number; question_set_count: number; preparation_duration_minutes: number };
export type Interview = { id: string; job_id: string; job_title: string; job_code: string; candidate_name: string; candidate_email: string; state: string; duration_minutes: number; created_at: string; expires_at: string; started_at: string | null; deadline_at: string | null; submitted_at: string | null; description?: string; question_count?: number; answer_count?: number; invitation_recoverable?: boolean; invitation_active?: boolean };
export type Audit = { id: string; action: string; entity_id: string | null; actor_id: string | null; created_at: string; metadata: Record<string, unknown> };
export type Summary = { jobs: number; sessions: number; active: number; reviewable: number };
export function workspaceApi(token: string) {
  const base = '/api/v1';
  const get = <T,>(path: string) => request<T>(`${base}${path}`, {}, token);
  const post = <T,>(path: string, data: unknown) => request<T>(`${base}${path}`, { method: 'POST', body: JSON.stringify(data) }, token);
  return {
    summary: () => get<Summary>('/summary'),
    jobs: (cursor?: string) => get<Page<Job>>(`/jobs${cursor ? `?cursor=${cursor}` : ''}`),
    sessions: (cursor?: string) => get<Page<Interview>>(`/sessions${cursor ? `?cursor=${cursor}` : ''}`),
    detail: (id: string) => get<Interview>(`/sessions/${id}`),
    generation: (id: string, signal?: AbortSignal) => request<Generation>(`${base}/sessions/${id}/generation`, { signal }, token),
    prepare: (id: string) => post<{ status: string }>(`/sessions/${id}/generation`, {}),
    invitation: (id: string) => post<{ invitation_token: string; expires_at: string; replaced_legacy_link: boolean }>(`/sessions/${id}/invitation`, {}),
    audit: (entity?: string, cursor?: string) => get<Page<Audit>>(`/audit?${new URLSearchParams({ ...(entity ? { entity_id: entity } : {}), ...(cursor ? { cursor } : {}) })}`),
    questionSets: (id: string, signal?: AbortSignal) => request<JobPool>(`${base}/jobs/${id}/question-sets`, { signal }, token),
    retryQuestionSets: (id: string) => post<{ status: string }>(`/jobs/${id}/question-sets`, {}),
    createJob: (data: { question_set_count: number; preparation_duration_minutes: number; job_code: string; title: string; description: string; request_key: string }) => post<Job>('/jobs', data),
    invite: (job: string, data: { candidate_name: string; candidate_email: string; duration_minutes: number; request_key: string }) => post<{ session: Interview; invitation_token: string | null }>(`/jobs/${job}/invitations`, data),
  };
}

// A stale API may omit the invitation flags. Missing data must never be treated
// as proof of expiry; the recovery endpoint still enforces server-side timing.
export function invitationIsActive(session: Interview): boolean {
  if (typeof session.invitation_active === 'boolean') return session.invitation_active;
  return ['invited', 'ready', 'in_progress'].includes(session.state)
    && Date.parse(session.expires_at) > Date.now()
    && (!session.deadline_at || Date.parse(session.deadline_at) > Date.now());
}

export type RubricCriterion = { id: string; description: string; max_points: number; partial_credit?: string; full_credit?: string; reference_evidence?: string; reference_source?: string };
export type PreparedQuestion = { id: string; position: number; prompt: string; kind: string; difficulty: string; competency_id: string; answer?: { content: string; revision: number; updated_at: string } | null; rubric: { expected_answer: string; estimated_minutes?: number; assumptions?: string[]; acceptable_alternatives?: string[]; common_mistakes?: string[]; coding?: { language: string; solution: string; complexity: string; tests: { category: string; input: string; expected_output: string; explanation: string }[] } | null; quality_review?: { reference_correct: boolean }; criteria: RubricCriterion[]; max_score: number } };
export type Generation = { status: 'not_started' | 'pending' | 'running' | 'completed' | 'failed'; configured: boolean; prompt_version: string; job: { id: string; status: string; attempts: number; last_error: string | null; seed: string } | null; blueprint: { competencies: { id: string; name: string; jd_evidence: string; weight: number }[]; slots: { competency_id: string; kind: string; difficulty: string }[] } | null; questions: PreparedQuestion[] };

export type PreparationFailure = { phase: string; summary: string; question_position: number | null; findings: { questions?: { position?: number; concerns?: string[]; jd_grounded?: boolean; reference_correct?: boolean; rubric_aligned?: boolean; feasible?: boolean; unambiguous?: boolean; no_answer_leak?: boolean }[] } | null };
export type JobPool = { configured: boolean; target: number; duration_minutes: number; sets: { id: string; status: string; duration_minutes: number; attempts: number; last_error: string | null; assigned: boolean; question_count: number; last_failure?: PreparationFailure | null }[]; questions: (PreparedQuestion & { set_id: string })[] };
