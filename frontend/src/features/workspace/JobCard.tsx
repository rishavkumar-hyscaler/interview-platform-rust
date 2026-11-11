import { ArrowUpRight, LoaderCircle } from 'lucide-react';
import { DashboardCard } from './DashboardCard';
import type { Job } from './api';

export function JobCard({
  job,
  index,
  onOpen,
  analytics = false,
}: {
  job: Job;
  index: number;
  onOpen: () => void;
  analytics?: boolean;
}) {
  return (
    <DashboardCard variant="job" index={index} className="h-full min-h-44">
      <div className="flex items-start justify-between gap-3">
        <span className="pill max-w-full break-all">{job.job_code}</span>
        <span
          aria-hidden="true"
          className="job-icon grid shrink-0 place-items-center motion-safe:transition-transform motion-safe:group-hover:translate-x-0.5 motion-safe:group-hover:-translate-y-0.5"
        >
          <ArrowUpRight className="size-4 text-violet-700 workspace-dark:text-violet-300" />
        </span>
      </div>
      <h3 className="mt-0! break-words">{job.title}</h3>
      {job.blueprint_preparation &&
        job.blueprint_preparation.status !== 'completed' && (
          <div
            className="flex items-center gap-2 text-detail text-indigo-600 workspace-dark:text-indigo-300"
            role="status"
          >
            {['pending', 'running'].includes(
              job.blueprint_preparation.status,
            ) && <LoaderCircle className="size-3.5 motion-safe:animate-spin" />}
            {job.blueprint_preparation.status === 'failed'
              ? 'Blueprint needs attention'
              : job.blueprint_preparation.status === 'not_started'
                ? 'Blueprint not prepared'
                : 'Preparing blueprint…'}
          </div>
        )}
      <div className="mt-auto grid gap-2 border-t border-slate-200/70 pt-3 text-control workspace-dark:border-slate-700/70">
        <div className="flex items-center justify-between gap-3">
          <span className="text-slate-500 workspace-dark:text-slate-400">
            Total invites
          </span>
          <span className="font-medium tabular-nums text-slate-700 workspace-dark:text-slate-200">
            {job.session_count}
          </span>
        </div>
        <div className="flex items-center justify-between gap-3 text-detail text-slate-500 workspace-dark:text-slate-400">
          <span>Created</span>
          <time dateTime={job.created_at}>
            {new Date(job.created_at).toLocaleDateString(undefined, {
              day: 'numeric',
              month: 'short',
              year: 'numeric',
            })}
          </time>
        </div>
      </div>
      <button
        type="button"
        className="absolute inset-0 cursor-pointer rounded-xl border-0 bg-transparent p-0 hover:bg-transparent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500"
        aria-label={`Open ${analytics ? 'analytics' : 'job details'} for ${job.job_code}: ${job.title}`}
        aria-haspopup={analytics ? undefined : 'dialog'}
        onClick={onOpen}
      />
    </DashboardCard>
  );
}
