import type { ComponentProps } from 'react';
import { cn } from '../../lib/utils';

export function Button({ className, type = 'button', ...props }: ComponentProps<'button'>) {
  return <button type={type} data-slot="button" className={cn(
    'inline-flex min-h-10 items-center justify-center gap-2 rounded-md border border-solid border-slate-300 bg-white px-4 text-sm font-medium text-slate-800 transition-colors hover:bg-slate-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500 disabled:pointer-events-none disabled:opacity-50 workspace-dark:border-slate-600 workspace-dark:bg-slate-800 workspace-dark:text-slate-100 workspace-dark:hover:bg-slate-700',
    className,
  )} {...props} />;
}
