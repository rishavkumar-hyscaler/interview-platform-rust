import { QuestionClipboard } from './proctoring/clipboard';
import { useEffect } from 'react';
import type { Observations } from './proctoring/observations';
export function Proctoring({ observations }: { observations: Observations }) {
  useEffect(() => {
    const record = observations.record;
    const blur = () => record('focus_lost');
    const focus = () => record('focus_returned');
    const visibility = () => record(document.hidden ? 'hidden' : 'visible');
    const full = () => {
      record(
        document.fullscreenElement ? 'fullscreen_entered' : 'fullscreen_exited',
      );
    };
    const clipboard = new QuestionClipboard();
    const question = () =>
      document.querySelector<HTMLElement>(
        '.question-content[data-question-id]',
      );
    const copy = () => {
      const root = question();
      const selection = window.getSelection();
      const local =
        !!root &&
        !!selection?.anchorNode &&
        !!selection.focusNode &&
        root.contains(selection.anchorNode) &&
        root.contains(selection.focusNode);
      if (
        local &&
        clipboard.copy(
          root.dataset.questionId ?? '',
          selection.toString(),
          root.innerText,
        )
      ) {
        record('copy_from_question', selection.toString().length);
      } else {
        clipboard.clear();
        record('copy');
      }
    };
    const cut = () => clipboard.clear();
    const paste = (event: ClipboardEvent) => {
      const text = event.clipboardData?.getData('text/plain') ?? '';
      const root = question();
      const allowed =
        !!root &&
        clipboard.matches(root.dataset.questionId ?? '', text, root.innerText);
      record(allowed ? 'paste_from_question' : 'paste', text.length);
    };
    const context = (event: MouseEvent) => {
      const root = question();
      record(
        root && event.target instanceof Node && root.contains(event.target)
          ? 'question_context_menu'
          : 'context_menu',
      );
    };
    let lastInput = performance.now();
    let rapidCharacters = 0;
    let lastBurst = 0;
    const input = (event: Event) => {
      const typed = event as InputEvent;
      if (typed.isComposing || typed.inputType !== 'insertText') return;
      const now = performance.now();
      rapidCharacters =
        now - lastInput < 20 ? rapidCharacters + (typed.data?.length ?? 0) : 0;
      lastInput = now;
      if (rapidCharacters >= 20 && now - lastBurst > 10000) {
        record('typing_burst', rapidCharacters);
        lastBurst = now;
        rapidCharacters = 0;
      }
    };
    let toolsSuspected = false;
    const inspect = () => {
      const suspected =
        Math.max(
          window.outerWidth - window.innerWidth,
          window.outerHeight - window.innerHeight,
        ) > 220;
      if (suspected && !toolsSuspected)
        record(
          'devtools_suspected',
          Math.max(
            window.outerWidth - window.innerWidth,
            window.outerHeight - window.innerHeight,
          ),
        );
      toolsSuspected = suspected;
    };
    const device = (event: Event) => {
      const detail = (
        event as CustomEvent<string | { kind: string; count?: number }>
      ).detail;
      if (typeof detail === 'string') record(detail);
      else record(detail.kind, detail.count);
    };
    document.addEventListener('input', input);
    window.addEventListener('resize', inspect);
    if ((window.screen as Screen & { isExtended?: boolean }).isExtended)
      record('multiple_displays', 2);
    record('monitoring_capability', 'isExtended' in window.screen ? 1 : 0);
    window.addEventListener('blur', blur);
    window.addEventListener('focus', focus);
    document.addEventListener('visibilitychange', visibility);
    document.addEventListener('fullscreenchange', full);
    document.addEventListener('copy', copy);
    document.addEventListener('cut', cut);
    document.addEventListener('paste', paste);
    document.addEventListener('contextmenu', context);
    window.addEventListener('interview-device', device);
    return () => {
      document.removeEventListener('input', input);
      window.removeEventListener('resize', inspect);
      window.removeEventListener('blur', blur);
      window.removeEventListener('focus', focus);
      document.removeEventListener('visibilitychange', visibility);
      document.removeEventListener('fullscreenchange', full);
      document.removeEventListener('copy', copy);
      document.removeEventListener('cut', cut);
      document.removeEventListener('paste', paste);
      document.removeEventListener('contextmenu', context);
      window.removeEventListener('interview-device', device);
    };
  }, [observations.record]);
  return (
    <div className="proctor-status">
      <small>
        {observations.status}. Activity signals are observations, not cheating
        verdicts.
      </small>
    </div>
  );
}
