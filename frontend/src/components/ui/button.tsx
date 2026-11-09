import type { ComponentProps } from 'react';
import { cn } from '../../lib/utils';

export function Button({ className, type = 'button', ...props }: ComponentProps<'button'>) {
  return <button type={type} data-slot="button" className={cn(
    'inline-flex min-h-10 items-center justify-center gap-2 rounded-xl px-4 text-control font-medium focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-500 disabled:pointer-events-none disabled:opacity-50',
    className,
  )} {...props} />;
}
