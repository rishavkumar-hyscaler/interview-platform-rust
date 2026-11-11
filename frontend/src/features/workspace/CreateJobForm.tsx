import type { FormEvent } from 'react';
import { LoaderCircle } from 'lucide-react';
import { Button } from '../../components/ui/button';
import { Input, Textarea } from '../../components/ui/input';
import { HelpTip } from './HelpTip';

export function CreateJobForm({
  onSubmit,
  onChange,
  onCancel,
  busy,
}: {
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onChange: () => void;
  onCancel: () => void;
  busy: boolean;
}) {
  return (
    <form
      className="flex! min-h-0 flex-col! gap-0!"
      onSubmit={onSubmit}
      onChange={onChange}
    >
      <div className="grid min-h-0 gap-4 overflow-y-auto px-6 py-5 sm:grid-cols-[1fr_160px]">
        <label className="grid gap-2 font-normal!">
          <span className="flex items-center gap-2">
            Job ID
            <HelpTip text="Unique for your account, ignoring case. Use letters, numbers, hyphens or underscores; start with a letter or number." />
          </span>
          <Input
            name="job_code"
            autoFocus
            required
            maxLength={64}
            autoComplete="off"
            placeholder="BE-2026-001"
          />
        </label>
        <label className="grid gap-2 font-normal!">
          Duration (min)
          <Input
            name="preparation_duration_minutes"
            type="number"
            min={5}
            max={180}
            defaultValue={10}
            required
          />
        </label>
        <label className="grid gap-2 font-normal! sm:col-span-2">
          Job title
          <Input
            name="title"
            required
            maxLength={160}
            placeholder="Senior Backend Engineer"
          />
        </label>
        <label className="grid gap-2 font-normal! sm:col-span-2">
          Job description
          <Textarea
            name="description"
            required
            minLength={20}
            maxLength={20000}
            rows={6}
            className="min-h-36"
            placeholder="Responsibilities and required skills…"
          />
        </label>
      </div>
      <div className="flex shrink-0 items-center justify-between gap-3 border-t border-slate-200 bg-slate-50/70 px-6 py-4 workspace-dark:border-slate-700 workspace-dark:bg-slate-950/20">
        <HelpTip text="The duration and competency blueprint are fixed when this job is created. Fresh questions are generated from that blueprint for each invited candidate." />
        <div className="flex items-center gap-2">
          <Button compact onClick={onCancel} disabled={busy}>
            Cancel
          </Button>
          <Button compact type="submit" disabled={busy}>
            {busy && (
              <LoaderCircle className="size-4 motion-safe:animate-spin" />
            )}
            {busy ? 'Preparing blueprint…' : 'Create job'}
          </Button>
        </div>
      </div>
    </form>
  );
}
