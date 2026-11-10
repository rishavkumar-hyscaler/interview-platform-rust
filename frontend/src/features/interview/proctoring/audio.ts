/** Keep at most three seconds locally; save only the window associated with a speech flag. */
export class SpeechWindow {
  private chunks: { samples: Float32Array; at: number }[] = [];
  constructor(private readonly sampleRate: number) {}
  push(samples: Float32Array, at: number) {
    this.chunks.push({ samples: samples.slice(), at });
    this.chunks = this.chunks.filter((chunk) => at - chunk.at < 3000).slice(-3);
  }
  capture(now: number): { blob: Blob; captured: number } | null {
    const chunks = this.chunks.filter((chunk) => now - chunk.at < 3000);
    if (!chunks.length) return null;
    const length = Math.min(
      this.sampleRate * 3,
      chunks.reduce((sum, chunk) => sum + chunk.samples.length, 0),
    );
    const bytes = new ArrayBuffer(44 + length * 2);
    const view = new DataView(bytes);
    const text = (offset: number, value: string) => {
      for (let i = 0; i < value.length; i++)
        view.setUint8(offset + i, value.charCodeAt(i));
    };
    text(0, 'RIFF');
    view.setUint32(4, 36 + length * 2, true);
    text(8, 'WAVE');
    text(12, 'fmt ');
    view.setUint32(16, 16, true);
    view.setUint16(20, 1, true);
    view.setUint16(22, 1, true);
    view.setUint32(24, this.sampleRate, true);
    view.setUint32(28, this.sampleRate * 2, true);
    view.setUint16(32, 2, true);
    view.setUint16(34, 16, true);
    text(36, 'data');
    view.setUint32(40, length * 2, true);
    let index = 0;
    for (const chunk of chunks)
      for (const sample of chunk.samples) {
        if (index >= length) break;
        const value = Math.max(-1, Math.min(1, sample));
        view.setInt16(
          44 + index++ * 2,
          value < 0 ? value * 32768 : value * 32767,
          true,
        );
      }
    const duration =
      (chunks.reduce((sum, chunk) => sum + chunk.samples.length, 0) /
        this.sampleRate) *
      1000;
    return {
      blob: new Blob([bytes], { type: 'audio/wav' }),
      captured: Date.now() - (now - chunks[chunks.length - 1].at) - duration,
    };
  }
  clear() {
    this.chunks = [];
  }
}
