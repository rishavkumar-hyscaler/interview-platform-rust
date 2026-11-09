import type { ComponentProps } from 'react';
import { cn } from '../../lib/utils';

export function Card({ className, ...props }: ComponentProps<'section'>) {
  return <section data-slot="card" className={cn(
    'grid gap-6 rounded-xl border border-solid border-slate-200 bg-white py-6 text-slate-900 shadow-sm workspace-dark:border-slate-700 workspace-dark:bg-slate-900 workspace-dark:text-slate-100',
    className,
  )} {...props} />;
}

export function CardHeader(props: ComponentProps<'header'>) {
  return <header data-slot="card-header" {...props} className={cn('grid gap-1.5 px-6', props.className)} />;
}

export function CardContent(props: ComponentProps<'div'>) {
  return <div data-slot="card-content" {...props} className={cn('px-6', props.className)} />;
}

export function CardFooter(props: ComponentProps<'div'>) {
  return <div data-slot="card-footer" {...props} className={cn('flex items-center px-6', props.className)} />;
}
