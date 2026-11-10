import { useEffect, useId, useRef } from 'react';

/** Explanation available on hover, keyboard focus, and touch. */
export function HelpTip({ text }: { text: string }) {
  const id = useId();
  const anchor = useRef<HTMLSpanElement>(null);
  const content = useRef<HTMLSpanElement>(null);
  const hideTimer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(hideTimer.current), []);

  function show() {
    window.clearTimeout(hideTimer.current);
    const tip = content.current;
    const trigger = anchor.current;
    if (!tip || !trigger) return;
    if (!tip.matches(':popover-open')) tip.showPopover();

    const origin = trigger.getBoundingClientRect();
    const margin = 12;
    const gap = 6;
    const below = window.innerHeight - origin.bottom - margin - gap;
    const above = origin.top - margin - gap;
    tip.style.maxHeight = '';
    const openAbove = tip.offsetHeight > below && above > below;
    tip.style.maxHeight = `${Math.max(0, openAbove ? above : below)}px`;
    tip.style.top = `${openAbove ? origin.top - gap - tip.offsetHeight : origin.bottom + gap}px`;
    tip.style.left = `${Math.max(margin, Math.min(origin.left, window.innerWidth - margin - tip.offsetWidth))}px`;
  }

  function scheduleHide() {
    window.clearTimeout(hideTimer.current);
    hideTimer.current = window.setTimeout(() => content.current?.hidePopover(), 150);
  }

  return <span ref={anchor} className="help-tip" onMouseEnter={show} onMouseLeave={scheduleHide}
    onFocus={show} onBlur={event => {
      if (!event.currentTarget.contains(event.relatedTarget)) scheduleHide();
    }}>
    <button type="button" className="help-tip-trigger" aria-label="More information" aria-describedby={id}
      onClick={event => { event.stopPropagation(); show(); }}>i</button>
    <span ref={content} id={id} popover="auto" role="tooltip"
      onMouseEnter={() => window.clearTimeout(hideTimer.current)}
      className="help-tip-content fixed inset-auto m-0 w-max max-w-[min(28rem,calc(100vw-24px))] overflow-y-auto overscroll-contain whitespace-pre-wrap break-words rounded-lg border border-slate-700 bg-slate-900 px-3 py-2 text-left text-control leading-normal text-white shadow-lg"
    >{text}</span>
  </span>;
}
