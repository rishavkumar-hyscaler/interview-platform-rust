import { request } from '../../api';
export type CandidateSession = { id: string; job_title: string; state: string; duration_minutes: number; question_count: number; answer_count: number; generation_status: string; preparation_available?: boolean; deadline_at: string | null; server_time: string; submitted_at: string | null };
export type Question = { id: string; position: number; prompt: string; kind: string; difficulty: string; answer: string; revision: number; saved_at: string | null };
export type Room = { session: CandidateSession; question: Question | null; question_started_at?: string };
export type SaveReceipt = { question_id: string; revision: number; saved_at: string; answer_count: number };
export function candidateApi(token: string) {
  return {
    session: (signal?: AbortSignal) => request<CandidateSession>('/api/v1/candidate/session', { signal }, token),
    transition: (action: 'ready' | 'start' | 'submit') => request<CandidateSession>(`/api/v1/candidate/session/${action}`, { method: 'POST' }, token),
    room: (position?: number) => request<Room>(`/api/v1/candidate/room${position ? `?position=${position}` : ''}`, {}, token),
    save: (id: string, content: string, revision: number) => request<SaveReceipt>(`/api/v1/candidate/answers/${id}`, { method: 'POST', body: JSON.stringify({ content, revision }) }, token),
  };
}
