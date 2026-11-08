import { useState, type ReactNode } from 'react';
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '../../components/ui/accordion';
import { HelpTip } from './HelpTip';

/** Independent sections can remain open together, including nested evidence. */
export function ExpandableSection({ label, children, className = '', help, open, onOpenChange }: {
  label: ReactNode;
  children: ReactNode;
  className?: string;
  help?: string;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  return <Accordion type="single" collapsible className={`expandable-section ${className}`}
    value={(open ?? expanded) ? 'content' : ''}
    onValueChange={value => { setExpanded(value === 'content'); onOpenChange?.(value === 'content'); }}>
    <AccordionItem value="content">
      <div className="flex items-center gap-2">
        <AccordionTrigger>{label}</AccordionTrigger>
        {help && <HelpTip text={help} />}
      </div>
      <AccordionContent>{children}</AccordionContent>
    </AccordionItem>
  </Accordion>;
}
