import { useId } from 'react';
/** Small explanation available on hover, keyboard focus and touch. */
export function HelpTip({ text }: { text: string }) {
  const id = useId();
  return <span className="help-tip"><button type="button" className="help-tip-trigger" aria-label="More information" aria-describedby={id} onClick={event => event.stopPropagation()}>?</button><span className="help-tip-content" role="tooltip" id={id}>{text}</span></span>;
}
