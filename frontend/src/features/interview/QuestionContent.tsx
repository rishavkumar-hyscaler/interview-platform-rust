import type { ReactNode } from 'react';

// Render the supported question formatting as React nodes, never injected HTML.
function inline(text: string): ReactNode[] {
  return text
    .split(/(`[^`]+`|\*\*[^*]+\*\*)/g)
    .map((part, i) =>
      part.startsWith('`') && part.endsWith('`') ? (
        <code key={i}>{part.slice(1, -1)}</code>
      ) : part.startsWith('**') && part.endsWith('**') ? (
        <strong key={i}>{part.slice(2, -2)}</strong>
      ) : (
        part
      ),
    );
}
export function QuestionContent({
  content,
  questionId,
}: {
  content: string;
  questionId?: string;
}) {
  const lines = content.replace(/\r\n/g, '\n').split('\n');
  const blocks: ReactNode[] = [];
  for (let index = 0; index < lines.length; ) {
    const line = lines[index];
    if (!line.trim()) {
      index++;
      continue;
    }
    if (line.trim().startsWith('```')) {
      const code: string[] = [];
      index++;
      while (index < lines.length && !lines[index].trim().startsWith('```'))
        code.push(lines[index++]);
      index++;
      blocks.push(
        <pre key={blocks.length}>
          <code>{code.join('\n')}</code>
        </pre>,
      );
      continue;
    }
    const list = line.match(/^\s*(?:([-*])\s+|\d+[.)]\s+)(.*)$/);
    if (list) {
      const ordered = !list[1];
      const items: ReactNode[] = [];
      const start = ordered ? Number.parseInt(line.trim(), 10) : undefined;
      while (index < lines.length) {
        const item = lines[index].match(/^\s*(?:([-*])\s+|\d+[.)]\s+)(.*)$/);
        if (!item || !item[1] !== ordered) break;
        items.push(<li key={items.length}>{inline(item[2])}</li>);
        index++;
      }
      blocks.push(
        ordered ? (
          <ol start={start} key={blocks.length}>
            {items}
          </ol>
        ) : (
          <ul key={blocks.length}>{items}</ul>
        ),
      );
      continue;
    }
    const heading = line.match(/^#{1,6}\s+(.*)$/);
    blocks.push(
      heading ? (
        <h3 key={blocks.length}>{inline(heading[1])}</h3>
      ) : (
        <p key={blocks.length}>{inline(line)}</p>
      ),
    );
    index++;
  }
  return (
    <div className="question-content" data-question-id={questionId}>
      {blocks}
    </div>
  );
}
