import { useLiveScreenSender } from '../live-screen/useLiveScreenSender';
import { useCallback, useEffect, useRef, useState } from 'react';
import { evidenceQueue } from './proctoring/evidence';
import { captureScreenFrame } from './proctoring/screenFrame';
import type {
  BrowserObservation,
  Observations,
} from './proctoring/observations';
const flags = new Set([
  'fullscreen_entered',
  'fullscreen_exited',
  'focus_lost',
  'hidden',
  'paste',
  'copy',
  'context_menu',
  'devtools_suspected',
  'multiple_displays',
  'typing_burst',
  'rapid_answer',
  'camera_disabled',
  'microphone_disabled',
  'screen_share_started',
  'face_missing',
  'multiple_faces',
  'gaze_away',
  'possible_background_speech',
  'speech_detected',
]);
export function useScreenEvidence(
  token: string,
  session: string | undefined,
  active: boolean,
  observations: Observations,
  cameraTrack: MediaStreamTrack | null,
) {
  const pickerOpen = useRef(false);
  const stream = useRef<MediaStream | null>(null);
  const video = useRef<HTMLVideoElement | null>(null);
  const queue = useRef<ReturnType<typeof evidenceQueue> | null>(null);
  const alive = useRef(true);
  const entered = useRef(false);
  const entryRecorded = useRef(false);
  const isActive = useRef(active);
  isActive.current = active;
  const [liveStream, setLiveStream] = useState<MediaStream | null>(null);
  const liveStatus = useLiveScreenSender(
    token,
    active,
    liveStream,
    cameraTrack,
  );
  const [shared, setShared] = useState(false);
  const [busy, setBusy] = useState(false);
  const [surface, setSurface] = useState('');
  const [error, setError] = useState('');
  const [status, setStatus] = useState('Screen sharing not started');
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      stream.current?.getTracks().forEach((track) => track.stop());
      if (video.current) video.current.srcObject = null;
    };
  }, []);
  useEffect(() => {
    if (!session) return;
    const next = evidenceQueue(token, session, setStatus);
    queue.current = next;
    return () => {
      next.stop();
      queue.current = null;
    };
  }, [token, session]);
  useEffect(() => {
    if (active) return;
    entered.current = false;
    entryRecorded.current = false;
  }, [active]);
  const stop = useCallback(() => {
    stream.current?.getTracks().forEach((track) => track.stop());
    stream.current = null;
    if (video.current) video.current.srcObject = null;
    setLiveStream(null);
    setShared(false);
    setStatus('Screen sharing stopped');
  }, []);
  const request = useCallback(async () => {
    if (pickerOpen.current) return;
    pickerOpen.current = true;
    setBusy(true);
    setError('');
    try {
      observations.record('screen_share_requested');
      if (!navigator.mediaDevices?.getDisplayMedia)
        throw new Error(
          'Screen sharing requires a supported desktop browser on localhost or HTTPS.',
        );
      const acquired = await navigator.mediaDevices.getDisplayMedia({
        video: true,
        audio: false,
      });
      if (!alive.current) {
        acquired.getTracks().forEach((track) => track.stop());
        return;
      }
      const track = acquired.getVideoTracks()[0];
      if (!track) {
        acquired.getTracks().forEach((track) => track.stop());
        throw new Error('No screen was shared.');
      }
      stream.current?.getTracks().forEach((old) => old.stop());
      stream.current = acquired;
      const preview = document.createElement('video');
      preview.muted = true;
      preview.playsInline = true;
      preview.srcObject = acquired;
      video.current = preview;
      await preview.play();
      const selected = track.getSettings().displaySurface ?? 'unknown';
      setSurface(selected);
      track.onended = () => {
        if (alive.current) {
          stop();
          observations.record('screen_share_stopped');
        }
      };
      setLiveStream(acquired);
      setShared(true);
      setStatus(
        'Screen sharing active · screenshots are saved on fullscreen entry and flagged events',
      );
      observations.record('screen_share_started');
    } catch (failure) {
      if (!alive.current) return;
      stop();
      const denied =
        failure instanceof DOMException && failure.name === 'NotAllowedError';
      observations.record(
        denied ? 'screen_share_denied' : 'screen_share_unavailable',
      );
      setError(
        denied
          ? 'Screen sharing was denied or cancelled. Share a screen to enable screenshot evidence.'
          : failure instanceof Error
            ? failure.message
            : 'Unable to share screen.',
      );
    } finally {
      pickerOpen.current = false;
      if (alive.current) setBusy(false);
    }
  }, [observations.record, stop]);
  useEffect(() => {
    async function capture(event: BrowserObservation) {
      if (
        !isActive.current ||
        !stream.current ||
        !video.current ||
        !queue.current
      )
        return;
      if (event.kind === 'fullscreen_entered') {
        entered.current = true;
        entryRecorded.current = true;
      }
      if (document.fullscreenElement) entered.current = true;
      if (!entered.current || !flags.has(event.kind)) return;
      try {
        const preview = video.current;
        const sharedStream = stream.current;
        const destination = queue.current;
        const track = sharedStream.getVideoTracks()[0];
        if (!track) throw new Error('Screen track unavailable');
        const fullscreenExit = event.kind === 'fullscreen_exited';
        if (fullscreenExit) {
          // Give the browser compositor and the interview's fullscreen gate time to update.
          await new Promise<void>((resolve) => window.setTimeout(resolve, 350));
          if (
            document.fullscreenElement ||
            document.visibilityState !== 'visible' ||
            !isActive.current ||
            preview !== video.current ||
            destination !== queue.current
          )
            return;
        }
        const { blob, captured } = await captureScreenFrame(track, preview, {
          waitForPresentedFrame: fullscreenExit,
        });
        if (
          fullscreenExit &&
          (document.fullscreenElement || document.visibilityState !== 'visible')
        )
          return;
        // A permission/source/session change must not attach a frame to the previous event.
        if (
          !isActive.current ||
          preview !== video.current ||
          sharedStream !== stream.current ||
          destination !== queue.current
        )
          return;
        const selected = track.getSettings().displaySurface ?? 'unknown';
        await destination.enqueue(event, [
          {
            id: crypto.randomUUID(),
            blob,
            source: 'screen',
            surface: selected,
            captured,
          },
        ]);
      } catch {
        observations.record('evidence_capture_failed');
        setStatus(
          'No fresh screen frame available. Screenshot omitted; event and camera evidence are retained.',
        );
      }
    }
    const unsubscribe = observations.subscribe((event) => {
      void capture(event);
    });
    const media = (event: Event) => {
      void capture((event as CustomEvent<BrowserObservation>).detail);
    };
    window.addEventListener('interview-media-observation', media);
    if (
      active &&
      shared &&
      document.fullscreenElement &&
      !entryRecorded.current
    )
      observations.record('fullscreen_entered');
    return () => {
      unsubscribe();
      window.removeEventListener('interview-media-observation', media);
    };
  }, [active, shared, observations.record, observations.subscribe]);
  return { shared, busy, surface, status, error, request, stop, liveStatus };
}
export type ScreenEvidence = ReturnType<typeof useScreenEvidence>;
