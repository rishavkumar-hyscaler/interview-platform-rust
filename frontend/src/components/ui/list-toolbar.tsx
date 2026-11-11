import {
  useId,
  useRef,
  useState,
  type ComponentProps,
  type ReactNode,
} from 'react';
import { ArrowDownWideNarrow, Funnel, Search } from 'lucide-react';
import { Button } from './button';
import { Input } from './input';
import { cn } from '../../lib/utils';

function ToolbarMenu({
  label,
  icon,
  children,
  active = false,
  closeOnChange = false,
}: {
  label: string;
  icon: ReactNode;
  children: ReactNode;
  active?: boolean;
  closeOnChange?: boolean;
}) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);

  function position() {
    const content = panel.current;
    const button = trigger.current;
    if (!content || !button || !content.matches(':popover-open')) return;
    const rect = button.getBoundingClientRect();
    content.style.left = `${Math.max(12, Math.min(rect.right - content.offsetWidth, window.innerWidth - content.offsetWidth - 12))}px`;
    content.style.top = `${Math.max(12, Math.min(rect.bottom + 8, window.innerHeight - content.offsetHeight - 12))}px`;
  }

  return (
    <>
      <Button
        ref={trigger}
        compact
        popoverTarget={id}
        aria-expanded={open}
        aria-controls={id}
        className={cn(
          'gap-1.5! rounded-lg! border-0! bg-transparent! px-2.5! text-control! font-normal! text-slate-600! shadow-none! hover:bg-slate-200/70! workspace-dark:text-slate-300! workspace-dark:hover:bg-white/5!',
          active && 'text-indigo-600! workspace-dark:text-indigo-300!',
        )}
      >
        {icon}
        {label}
        {active && (
          <span
            className="size-1.5 rounded-full bg-indigo-500"
            aria-label="Active filters"
          />
        )}
      </Button>
      <div
        id={id}
        ref={panel}
        popover="auto"
        role="dialog"
        aria-label={label}
        onToggle={() => {
          setOpen(panel.current?.matches(':popover-open') ?? false);
          position();
        }}
        onChange={() => {
          if (closeOnChange) panel.current?.hidePopover();
        }}
        className="fixed inset-auto m-0 max-h-[70dvh] w-72 max-w-[calc(100vw-24px)] overflow-y-auto rounded-xl border border-slate-200 bg-white p-4 text-slate-700 shadow-xl workspace-dark:border-slate-700 workspace-dark:bg-slate-900 workspace-dark:text-slate-200"
      >
        <div className="grid gap-3 [&_label]:gap-1.5 [&_label]:font-normal!">
          {children}
        </div>
      </div>
    </>
  );
}

/** Shared search, sort and filter surface across workspace lists. */
export function ListToolbar({
  search,
  sort,
  filters,
  filtersActive = false,
  actions,
  className,
}: {
  search: ComponentProps<typeof Input>;
  sort: ReactNode;
  filters: ReactNode;
  filtersActive?: boolean;
  actions?: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        'flex min-w-0 flex-wrap items-center gap-2 rounded-xl border border-slate-200/70 bg-slate-100/80 px-2 py-1.5 workspace-dark:border-slate-700/50 workspace-dark:bg-white/[0.04]',
        className,
      )}
    >
      <div className="relative min-w-32 flex-1">
        <Search
          aria-hidden="true"
          className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-slate-400"
        />
        <Input
          {...search}
          onKeyDown={(event) => {
            search.onKeyDown?.(event);
            if (
              !event.defaultPrevented &&
              event.key === 'Enter' &&
              !event.nativeEvent.isComposing &&
              event.currentTarget.form
            ) {
              event.preventDefault();
              event.currentTarget.form.requestSubmit();
            }
          }}
          className={cn(
            'border-0! bg-transparent! pl-9! shadow-none! focus-visible:outline-offset-0 workspace-dark:bg-transparent!',
            search.className,
          )}
        />
      </div>
      <div className="ml-auto flex flex-wrap items-center gap-2">
        {actions}
        <div className="flex items-center gap-1">
          <ToolbarMenu
            label="Sort by"
            icon={<ArrowDownWideNarrow className="size-3.5" />}
            closeOnChange
          >
            {sort}
          </ToolbarMenu>
          <ToolbarMenu
            label="Filters"
            icon={<Funnel className="size-3.5" />}
            active={filtersActive}
          >
            {filters}
          </ToolbarMenu>
        </div>
      </div>
    </div>
  );
}
