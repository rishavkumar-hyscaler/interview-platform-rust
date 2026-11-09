type FrameCapture = { grabFrame(): Promise<ImageBitmap> };
type FrameCaptureConstructor = new (track: MediaStreamTrack) => FrameCapture;

/** Never stamp an already-decoded, potentially frozen preview frame as new evidence. */
export async function captureScreenFrame(track: MediaStreamTrack, preview: HTMLVideoElement, { waitForPresentedFrame = false } = {}): Promise<{ blob: Blob; captured: number }> {
  if (track.readyState !== 'live' || track.muted) throw new Error('Screen track is not live');
  const Capture = (window as unknown as { ImageCapture?: FrameCaptureConstructor }).ImageCapture;
  let frame: ImageBitmap | null = null;
  let expired = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let callback: number | undefined;
  try {
    const freshFrame = Capture && !waitForPresentedFrame
      ? new Capture(track).grabFrame().then(bitmap => {
          if (expired) { bitmap.close(); throw new Error('Screen capture expired'); }
          frame = bitmap;
          return bitmap;
        })
      : new Promise<HTMLVideoElement>((resolve, reject) => {
          if (typeof preview.requestVideoFrameCallback !== 'function') {
            reject(new Error('Fresh screen capture is unsupported'));
            return;
          }
          // Wait for a newly presented frame, not loadeddata/readyState from an earlier flag.
          callback = preview.requestVideoFrameCallback(() => resolve(preview));
        });
    const source = await Promise.race([
      freshFrame,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => { expired = true; reject(new Error('No fresh screen frame')); }, 1800);
      }),
    ]);
    if (track.readyState !== 'live' || track.muted || preview.srcObject === null) throw new Error('Screen sharing ended');
    const width = source instanceof HTMLVideoElement ? source.videoWidth : source.width;
    const height = source instanceof HTMLVideoElement ? source.videoHeight : source.height;
    if (!width || !height) throw new Error('Screen frame is empty');
    const captured = Date.now();
    const scale = Math.min(1, 1280 / width, 900 / height);
    const canvas = document.createElement('canvas');
    canvas.width = Math.round(width * scale);
    canvas.height = Math.round(height * scale);
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Screen canvas unavailable');
    context.drawImage(source, 0, 0, canvas.width, canvas.height);
    const blob = await new Promise<Blob | null>(resolve => canvas.toBlob(resolve, 'image/jpeg', 0.65));
    if (!blob || blob.size > 512 * 1024) throw new Error('Screen image unavailable or too large');
    return { blob, captured };
  } finally {
    expired = true;
    clearTimeout(timer);
    if (callback !== undefined) preview.cancelVideoFrameCallback(callback);
    (frame as ImageBitmap | null)?.close();
  }
}
