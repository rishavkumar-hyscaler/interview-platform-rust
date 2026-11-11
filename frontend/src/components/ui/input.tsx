import type { ComponentProps } from 'react';
import { cn } from '../../lib/utils';

export const fieldControl = [
  'block w-full min-w-0 rounded-lg! border border-solid border-slate-200! bg-slate-50/70!',
  'px-3! py-2! text-control! font-normal! text-slate-700! shadow-xs',
  'placeholder:text-slate-400 focus-visible:border-indigo-400! focus-visible:outline-indigo-300!',
  'disabled:cursor-not-allowed disabled:opacity-50 motion-safe:transition-colors',
  'workspace-dark:border-slate-700! workspace-dark:bg-slate-950/40! workspace-dark:text-slate-200!',
].join(' ');

export function Input({ className, ...props }: ComponentProps<'input'>) {
  return (
    <input
      data-slot="input"
      className={cn(fieldControl, 'h-[38px]! py-0! leading-normal!', className)}
      {...props}
    />
  );
}

export function Textarea({ className, ...props }: ComponentProps<'textarea'>) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(fieldControl, 'resize-y leading-relaxed', className)}
      {...props}
    />
  );
}
