import type { ComponentProps } from 'react';
import { cn } from '../../lib/utils';
import { fieldControl } from './input';

export function Select({ className, ...props }: ComponentProps<'select'>) {
  return (
    <select
      data-slot="select"
      className={cn(
        fieldControl,
        'h-[38px]! py-0! leading-normal! cursor-pointer pr-8! [&>option]:bg-white workspace-dark:[&>option]:bg-slate-900',
        className,
      )}
      {...props}
    />
  );
}
