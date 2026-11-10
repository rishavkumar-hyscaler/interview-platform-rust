import { RefreshCw } from 'lucide-react';
import type { ComponentProps } from 'react';
import { cn } from '../../lib/utils';
import { Button } from './button';

export function RefreshButton({
  pending,
  disabled,
  children,
  className,
  ...props
}: Omit<ComponentProps<typeof Button>, 'children'> & {
  pending: boolean;
  children: string;
}) {
  return (
    <span className="inline-flex" title={children}>
      <Button
        {...props}
        className={cn('size-[38px] min-h-[38px] p-0', className)}
        disabled={disabled || pending}
        aria-label={children}
        aria-busy={pending}
      >
        <RefreshCw
          size={16}
          aria-hidden="true"
          className={cn(
            'text-sky-700 workspace-dark:text-sky-300',
            pending && 'motion-safe:animate-spin',
          )}
        />
      </Button>
    </span>
  );
}
