import type { CSSProperties } from 'react';
import { cn } from '../../lib/utils';

export function RollingText({ text, className }: { text: string; className?: string }) {
  return <span className={cn('inline-flex whitespace-nowrap', className)}>
    <span className="sr-only">{text}</span>
    <span aria-hidden="true" className="inline-flex overflow-hidden leading-[1.4]">
      {Array.from(text, (letter, index) => <span
        key={index}
        style={{ '--roll-delay': `${Math.min(index, 12) * 16}ms` } as CSSProperties}
        className="block shrink-0 whitespace-pre [text-shadow:0_1.4em_currentColor] motion-safe:transition-transform motion-safe:duration-200 motion-safe:ease-out motion-safe:group-hover/rolling:-translate-y-full motion-safe:group-hover/rolling:delay-(--roll-delay) motion-safe:group-focus-visible/rolling:-translate-y-full motion-safe:group-focus-visible/rolling:delay-(--roll-delay)"
      >{letter}</span>)}
    </span>
  </span>;
}
