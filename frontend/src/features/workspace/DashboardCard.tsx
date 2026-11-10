import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { cn } from '../../lib/utils';

const variants = {
  metric: 'stat',
  job: 'job-card group relative gap-3',
  analytics: 'job-card',
  chart:
    'chart-card m-0 flex h-72 min-w-0 flex-col rounded-lg border border-slate-200 p-3 workspace-dark:border-slate-700',
};

export function DashboardCard({
  variant,
  index,
  children,
  className,
}: {
  variant: keyof typeof variants;
  index: number;
  children: ReactNode;
  className?: string;
}) {
  const Element = variant === 'chart' ? 'figure' : 'article';
  return (
    <Element
      className={cn(
        'motion-safe:animate-dashboard-card',
        variants[variant],
        className,
        (variant === 'job' || variant === 'analytics') &&
          'hover:border-indigo-400! focus-within:border-indigo-400! motion-safe:transition-[translate,box-shadow,border-color]! motion-safe:hover:-translate-y-1',
      )}
      style={
        { '--card-delay': `${Math.min(index, 5) * 55}ms` } as CSSProperties
      }
    >
      {children}
    </Element>
  );
}

export function AnimatedCount({ value }: { value: number | null }) {
  const [display, setDisplay] = useState(0);

  useEffect(() => {
    if (value === null) return;
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
    if (reducedMotion.matches || value === 0) {
      setDisplay(value);
      return;
    }

    const start = performance.now();
    let frame: number;
    function tick(now: number) {
      const progress = Math.min((now - start) / 480, 1);
      setDisplay(Math.round((value ?? 0) * (1 - (1 - progress) ** 3)));
      if (progress < 1) frame = requestAnimationFrame(tick);
    }

    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [value]);

  return (
    <strong className="tabular-nums">
      <span aria-hidden="true">{value === null ? '—' : display}</span>
      <span className="sr-only">{value ?? 'Loading'}</span>
    </strong>
  );
}
