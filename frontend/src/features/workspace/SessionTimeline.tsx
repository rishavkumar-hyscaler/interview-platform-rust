import { keepPreviousData, useQuery } from '@tanstack/react-query';
import { useMemo, useRef, useState } from 'react';
import { AuditTable } from './AuditTable';
import { auditPresentation } from './auditPresentation';
import { workspaceApi, type AuditPage, type Interview } from './api';
import { ListToolbar } from '../../components/ui/list-toolbar';
import { Pagination } from '../../components/ui/pagination';
import { Select } from '../../components/ui/select';
import { Button } from '../../components/ui/button';
import { RefreshButton } from '../../components/ui/refresh-button';

export function SessionTimeline({
  session,
  token,
  active,
}: {
  session: Interview;
  token: string;
  active: boolean;
}) {
  const api = useMemo(() => workspaceApi(token), [token]);
  const [cursors, setCursors] = useState<string[]>([]);
  const [search, setSearch] = useState('');
  const [sort, setSort] = useState('newest');
  const [category, setCategory] = useState('all');
  const scroll = useRef<HTMLDivElement>(null);
  const query = useQuery<AuditPage>({
    queryKey: ['session-timeline', token, session.id, cursors.at(-1)],
    queryFn: ({ signal }) => api.audit(session.id, cursors.at(-1), {}, signal),
    enabled: active,
    placeholderData: keepPreviousData,
    refetchInterval:
      active && cursors.length === 0 && session.state === 'in_progress'
        ? 5000
        : false,
  });
  const page = query.data;
  const events = (page?.items ?? [])
    .filter((event) => {
      const presentation = auditPresentation(event);
      const matches =
        `${presentation.title} ${presentation.performedBy} ${presentation.details.join(' ')}`
          .toLowerCase()
          .includes(search.trim().toLowerCase());
      const group =
        category === 'all' ||
        (category === 'scoring'
          ? event.action.startsWith('assessment.')
          : category === 'question' ? event.action.startsWith('question') : event.action.startsWith(`${category}.`));
      return matches && group;
    })
    .sort(
      (a, b) =>
        (Date.parse(b.created_at) - Date.parse(a.created_at)) *
        (sort === 'oldest' ? -1 : 1),
    );

  return (
    <div className="overflow-hidden rounded-xl border border-slate-200 workspace-dark:border-slate-700">
      <div className="flex items-center justify-between gap-3 px-5 pt-4">
        <h3 className="m-0!">Session timeline</h3>
        <RefreshButton
          pending={query.isFetching}
          onClick={() => void query.refetch()}
        >
          Refresh timeline
        </RefreshButton>
      </div>
      <ListToolbar
        className="mx-5 my-4"
        search={{
          'aria-label': 'Search session events on this page',
          placeholder: 'Search this page…',
          value: search,
          onChange: (event) => setSearch(event.target.value),
        }}
        filtersActive={category !== 'all'}
        sort={
          <label>
            Sort this page
            <Select
              value={sort}
              onChange={(event) => setSort(event.target.value)}
            >
              <option value="newest">Newest first</option>
              <option value="oldest">Oldest first</option>
            </Select>
          </label>
        }
        filters={
          <label>
            Events on this page
            <Select
              value={category}
              onChange={(event) => setCategory(event.target.value)}
            >
              <option value="all">All events</option>
              <option value="session">Interview activity</option>
              <option value="question">Question activity</option>
              <option value="answer">Answers</option>
              <option value="scoring">Scoring</option>
              <option value="proctor">Proctoring</option>
              <option value="reviewer">Reviewer activity</option>
            </Select>
          </label>
        }
      />
      <div
        ref={scroll}
        className="max-h-[55dvh] overflow-auto [scrollbar-width:thin]"
        role="region"
        aria-label="Session event history"
        tabIndex={0}
      >
        {query.isPending ? (
          <p className="px-5 py-6" role="status">
            Loading session history…
          </p>
        ) : query.isError ? (
          <div className="grid justify-items-center gap-3 p-5" role="alert">
            <p>Unable to load session history.</p>
            <Button compact onClick={() => void query.refetch()}>
              Retry
            </Button>
          </div>
        ) : (
          <AuditTable events={events} compact />
        )}
      </div>
      <Pagination
        label="Session timeline pagination"
        page={cursors.length + 1}
        summary={`${events.length} on this page${page?.total !== undefined ? ` · ${page.total} events total` : ''}`}
        hasNext={!!page?.next_cursor}
        busy={query.isFetching}
        onChange={(direction) => {
          if (direction === 'next' && page?.next_cursor)
            setCursors([...cursors, page.next_cursor]);
          else if (direction === 'previous') setCursors(cursors.slice(0, -1));
          scroll.current?.scrollTo(0, 0);
        }}
      />
    </div>
  );
}
