import { useId, useState } from 'react';

const dayLabel = new Intl.DateTimeFormat(undefined, {
  month: 'short',
  day: 'numeric',
  timeZone: 'UTC',
});

export function MetricTrend({
  counts,
  endDate,
  label,
}: {
  counts?: number[];
  endDate?: string;
  label: string;
}) {
  const gradient = useId();
  const [hoveredDay, setHoveredDay] = useState<number | null>(null);
  if (counts?.length !== 7 || !endDate) return null;

  const maximum = Math.max(1, ...counts);
  const points = counts.map((count, index) => {
    const day = new Date(`${endDate}T00:00:00Z`);
    day.setUTCDate(day.getUTCDate() - (6 - index));
    return {
      x: 4 + index * 32,
      y: 43 - (count / maximum) * 38,
      day: dayLabel.format(day),
      count,
    };
  });
  const [first, ...rest] = points;
  const line = rest.reduce((path, point, index) => {
    const previous = points[index];
    const midpoint = (previous.x + point.x) / 2;
    return `${path} C ${midpoint},${previous.y} ${midpoint},${point.y} ${point.x},${point.y}`;
  }, `M ${first.x},${first.y}`);
  const active = hoveredDay === null ? null : points[hoveredDay];

  return (
    <figure className="m-0 mt-2">
      <div className="relative" onPointerLeave={() => setHoveredDay(null)}>
        <svg
          className="block h-12 w-full text-indigo-600 workspace-dark:text-indigo-400"
          viewBox="0 0 200 48"
          preserveAspectRatio="none"
          role="img"
          aria-label={`${label}, daily in UTC. ${points.map((point) => `${point.day}: ${point.count}`).join(', ')}`}
        >
          <defs>
            <linearGradient id={gradient} x2="0%" y2="100%">
              <stop stopColor="currentColor" stopOpacity=".25" />
              <stop offset="100%" stopColor="currentColor" stopOpacity="0" />
            </linearGradient>
          </defs>
          <path d={`${line} V 48 H ${first.x} Z`} fill={`url(#${gradient})`} />
          <path
            d={line}
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            vectorEffect="non-scaling-stroke"
          />
          {active && (
            <line
              x1={active.x}
              x2={active.x}
              y1="0"
              y2="48"
              stroke="currentColor"
              opacity=".4"
            />
          )}
          {points.map((point, index) => {
            const left = index === 0 ? 0 : point.x - 16;
            const right = index === points.length - 1 ? 200 : point.x + 16;
            return (
              <rect
                key={point.x}
                x={left}
                width={right - left}
                height="48"
                fill="transparent"
                onPointerEnter={() => setHoveredDay(index)}
              />
            );
          })}
        </svg>
        {active && (
          <div className="pointer-events-none absolute bottom-full right-0 z-10 mb-1 rounded-md bg-slate-900 px-2 py-1 text-detail text-white shadow-md">
            {active.day} · {active.count}
          </div>
        )}
      </div>
      <figcaption className="mt-1 text-detail text-slate-500 workspace-dark:text-slate-400">
        {label} · Last 7 days
      </figcaption>
    </figure>
  );
}
