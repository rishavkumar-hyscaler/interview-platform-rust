import { ExpandableSection } from './ExpandableSection';
import { useState } from 'react';
import { request } from '../../api';
type Stats = { notice: string; groups: { kind: string; difficulty: string; competency_id: string; sample_size: number; mean_points: number; score_stddev: number | null; item_total_correlation: number | null }[] };
export function Analytics({ job, token }: { job: string; token: string }) {
  const [stats, setStats] = useState<Stats | null>(null); const [error, setError] = useState('');
  return <ExpandableSection onOpenChange={open => { if (open && !stats) void request<Stats>(`/api/v1/jobs/${job}/analytics`, {}, token).then(setStats).catch(() => setError('Unable to load analytics. Close and reopen to retry.')); }} label="Question-bank analytics">{error && <p role="alert">{error}</p>}{stats ? <><p className="muted">{stats.notice}</p>{stats.groups.length ? stats.groups.map((group, i) => <p key={i}>{group.competency_id} · {group.kind} · {group.difficulty}<br />{group.sample_size} scored answers · mean {Number(group.mean_points).toFixed(1)}/10 · spread {group.score_stddev == null ? 'insufficient sample' : Number(group.score_stddev).toFixed(1)}<br />Item–total correlation: {group.sample_size < 20 || group.item_total_correlation == null ? 'insufficient sample' : Number(group.item_total_correlation).toFixed(2)}</p>) : <p>No scored answers yet.</p>}</> : !error && <p>Loading statistics…</p>}</ExpandableSection>;
}
