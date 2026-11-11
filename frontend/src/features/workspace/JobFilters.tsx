import type { FormEvent } from 'react';
import { Search, SlidersHorizontal, X } from 'lucide-react';
import { Input } from '../../components/ui/input';
import { Select } from '../../components/ui/select';
import { Button } from '../../components/ui/button';
import type { JobListFilters } from './api';

export function JobFilters({
  value,
  onApply,
}: {
  value: JobListFilters;
  onApply: (filters: JobListFilters) => void;
}) {
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    onApply({
      search: String(data.get('search') ?? '').trim(),
      invitations: String(data.get('invitations') ?? 'all'),
      sort: String(data.get('sort') ?? 'newest'),
    });
  }
  const active =
    !!value.search || value.invitations !== 'all' || value.sort !== 'newest';
  return (
    <form
      key={JSON.stringify(value)}
      onSubmit={submit}
      className="grid! grid-cols-1! items-center! gap-3! lg:grid-cols-[minmax(0,1fr)_auto]! border-b border-slate-200 px-5 py-4 workspace-dark:border-slate-700/70"
      aria-label="Filter jobs"
    >
      <div className="relative flex h-[38px] min-w-0 w-full max-w-md items-center">
        <Search
          aria-hidden="true"
          className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 size-4 text-slate-400"
        />
        <Input
          name="search"
          aria-label="Search jobs by ID or title"
          placeholder="Search jobs…"
          maxLength={160}
          defaultValue={value.search}
          className="pl-9!"
        />
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-2 lg:justify-self-end">
        <Select
          name="invitations"
          aria-label="Filter jobs by invitations"
          defaultValue={value.invitations}
          className="w-auto!"
        >
          <option value="all">All invitations</option>
          <option value="with">With invitations</option>
          <option value="without">No invitations</option>
        </Select>
        <Select
          name="sort"
          aria-label="Sort jobs"
          defaultValue={value.sort}
          className="w-auto!"
        >
          <option value="newest">Newest first</option>
          <option value="oldest">Oldest first</option>
        </Select>
        <Button compact type="submit">
          <SlidersHorizontal className="size-4" />
          Apply
        </Button>
        {active && (
          <Button
            compact
            onClick={() =>
              onApply({ search: '', invitations: 'all', sort: 'newest' })
            }
          >
            <X className="size-4" />
            Clear
          </Button>
        )}
      </div>
    </form>
  );
}
