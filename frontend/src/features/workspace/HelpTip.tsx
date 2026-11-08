import { useId, useRef } from 'react';
/** Small explanation available on hover, keyboard focus and touch. */
export function HelpTip({ text }: { text: string }) {
  const id = useId();
  const anchor = useRef<HTMLSpanElement>(null);
  const content = useRef<HTMLSpanElement>(null);
  function position() {
    if (!anchor.current || !content.current) return;
    const origin = anchor.current.getBoundingClientRect();
    const bounds = anchor.current.closest('dialog')?.getBoundingClientRect();
    const left = Math.max(8, (bounds?.left ?? 0) + 12);
    const right = Math.min(window.innerWidth - 8, (bounds?.right ?? window.innerWidth) - 12);
    const tip = content.current;
    tip.style.maxWidth = `${Math.max(0, right - left)}px`;
    const width = tip.getBoundingClientRect().width;
    // Keep the explanation inside the scrolling dialog rather than extending left of it.
    tip.style.left = `${Math.max(left, Math.min(origin.left, right - width)) - origin.left}px`;
  }
  return <span ref={anchor} className="help-tip" onMouseEnter={position} onFocus={position}><button type="button" className="help-tip-trigger" aria-label="More information" aria-describedby={id} onClick={event => { event.stopPropagation(); position(); }}>?</button><span ref={content} className="help-tip-content" role="tooltip" id={id}>{text}</span></span>;
}
