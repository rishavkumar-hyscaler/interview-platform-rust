import { Button } from './button';

type Direction = 'previous' | 'next';

export function Pagination({ label, page, summary, hasNext, busy, onChange }: {
  label: string;
  page: number;
  summary: string;
  hasNext: boolean;
  busy: boolean;
  onChange: (direction: Direction) => void;
}) {
  return <nav aria-label={label}
    className="flex flex-wrap items-center justify-between gap-3 border-t border-slate-200 px-4 py-3 sm:px-6 workspace-dark:border-slate-700">
    <span className="text-control tabular-nums" aria-live="polite">{summary}</span>
    <div className="flex flex-wrap items-center gap-2">
      <Button disabled={busy || page <= 1} onClick={() => onChange('previous')}>← Previous</Button>
      <span className="px-2 text-control tabular-nums" aria-current="page">Page {page}</span>
      <Button disabled={busy || !hasNext} onClick={() => onChange('next')}>Next →</Button>
    </div>
  </nav>;
}
