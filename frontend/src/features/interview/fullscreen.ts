/** Fullscreen must be requested directly from a click, before starting the timer. */
export async function enterInterviewFullscreen(): Promise<void> {
  if (document.fullscreenElement) return;
  if (
    !document.fullscreenEnabled ||
    !document.documentElement.requestFullscreen
  ) {
    throw new Error(
      'Fullscreen is required. Open this interview directly in a desktop browser that supports fullscreen.',
    );
  }
  try {
    await document.documentElement.requestFullscreen();
  } catch {
    throw new Error(
      'Fullscreen was blocked. Click Enter fullscreen again and allow fullscreen to continue.',
    );
  }
}
