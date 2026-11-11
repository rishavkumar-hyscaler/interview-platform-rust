import type { FormEvent, ReactNode } from 'react';
import { ListToolbar } from '../../components/ui/list-toolbar';
import { Select } from '../../components/ui/select';
import { Button } from '../../components/ui/button';
import type { JobListFilters } from './api';

export function JobFilters({
  value,
  onApply,
  actions,
}: {
  value: JobListFilters;
  actions?: ReactNode;
  onApply: (filters: JobListFilters) => void;
}) {
  function apply(form: HTMLFormElement) {
    const data = new FormData(form);
    onApply({
      search: String(data.get('search') ?? '').trim(),
      invitations: String(data.get('invitations') ?? 'all'),
      sort: String(data.get('sort') ?? 'newest'),
    });
  }
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    apply(event.currentTarget);
  }
  return (
    <form
      key={JSON.stringify(value)}
      onSubmit={submit}
      onChange={(event) => {
        if (
          event.target instanceof HTMLSelectElement &&
          event.target.name === 'sort'
        )
          apply(event.currentTarget);
      }}
      className="block! px-5 py-4"
      aria-label="Filter jobs"
    >
      <ListToolbar
        actions={actions}
        search={{
          name: 'search',
          'aria-label': 'Search jobs by ID or title',
          placeholder: 'Search jobs…',
          maxLength: 160,
          defaultValue: value.search,
        }}
        filtersActive={value.invitations !== 'all'}
        sort={
          <label>
            Sort jobs
            <Select name="sort" defaultValue={value.sort}>
              <option value="newest">Newest first</option>
              <option value="oldest">Oldest first</option>
            </Select>
          </label>
        }
        filters={
          <>
            <label>
              Invitations
              <Select name="invitations" defaultValue={value.invitations}>
                <option value="all">All invitations</option>
                <option value="with">With invitations</option>
                <option value="without">No invitations</option>
              </Select>
            </label>
            <div className="flex justify-end gap-2">
              <Button
                compact
                onClick={() =>
                  onApply({ search: '', invitations: 'all', sort: 'newest' })
                }
              >
                Clear
              </Button>
              <Button compact type="submit">
                Apply
              </Button>
            </div>
          </>
        }
      />
    </form>
  );
}
