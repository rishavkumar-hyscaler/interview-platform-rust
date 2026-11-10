/** Keep only the last locally observed question selection, in memory. Never upload clipboard text. */
const normalize = (text: string) => text.replace(/\s+/gu, ' ').trim();
export class QuestionClipboard {
  private copied: { question: string; text: string } | null = null;
  clear() {
    this.copied = null;
  }
  copy(question: string, selection: string, questionText: string): boolean {
    this.clear();
    const text = normalize(selection);
    if (!question || !text || !normalize(questionText).includes(text))
      return false;
    this.copied = { question, text };
    return true;
  }
  matches(question: string, pasted: string, questionText: string): boolean {
    const text = normalize(pasted);
    return (
      !!this.copied &&
      this.copied.question === question &&
      !!text &&
      this.copied.text === text &&
      normalize(questionText).includes(text)
    );
  }
}
