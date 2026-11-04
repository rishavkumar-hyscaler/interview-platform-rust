export type Draft = { content: string; revision: number };
const prefix = (session: string) => `interview:draft:${session}:`;
export function readDraft(session: string, question: string): Draft | null {
  try {
    const raw = localStorage.getItem(prefix(session) + question);
    if (!raw) return null;
    const value: unknown = JSON.parse(raw);
    if (value && typeof value === 'object' && 'content' in value && 'revision' in value && typeof value.content === 'string' && Number.isInteger(value.revision)) return value as Draft;
  } catch { /* Browser storage may be disabled. */ }
  return null;
}
export function writeDraft(session: string, question: string, draft: Draft): boolean {
  try { localStorage.setItem(prefix(session) + question, JSON.stringify(draft)); return true; } catch { return false; }
}
export function clearDraft(session: string, question: string, acknowledgedContent: string): void {
  try {
    if (readDraft(session, question)?.content === acknowledgedContent) localStorage.removeItem(prefix(session) + question);
  } catch { /* Keep any draft written by another tab. */ }
}
export function pendingDrafts(session: string): { question: string; content: string }[] {
  try {
    return Object.keys(localStorage).filter(key => key.startsWith(prefix(session))).flatMap(key => {
      const question = key.slice(prefix(session).length); const draft = readDraft(session, question);
      return draft ? [{ question, content: draft.content }] : [];
    });
  } catch { return []; }
}
