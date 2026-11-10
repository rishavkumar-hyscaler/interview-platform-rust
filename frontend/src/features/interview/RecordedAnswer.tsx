import { Button } from '../../components/ui/button';
import { startTask } from '../../observability';
import { loggedFetch } from '../../transport';
import { useEffect, useRef, useState } from 'react';
import {
  readRecording,
  recordingKey,
  removeRecording,
  storeRecording,
  type RecordingDraft,
} from './recordingStore';
export function RecordedAnswer({
  token,
  session,
  question,
  onPending,
  fullscreen,
}: {
  fullscreen: boolean;
  token: string;
  session: string;
  question: string;
  onPending: (pending: boolean) => void;
}) {
  const recorder = useRef<MediaRecorder | null>(null);
  const sending = useRef(false);
  const stream = useRef<MediaStream | null>(null);
  const draftRef = useRef<RecordingDraft | null>(null);
  const alive = useRef(true);
  const pending = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [draft, setDraft] = useState<RecordingDraft | null>(null);
  const [recording, setRecording] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [status, setStatus] = useState('');
  const [url, setUrl] = useState('');
  function keep(value: boolean) {
    pending.current = value;
    onPending(value);
  }
  useEffect(() => {
    alive.current = true;
    let cancelled = false;
    keep(true);
    void readRecording(session, question)
      .then((value) => {
        if (!cancelled && value) {
          draftRef.current = value;
          setDraft(value);
          keep(true);
          setStatus('Recovered recording · upload to submit');
        } else if (!cancelled) keep(false);
      })
      .catch(() => {
        if (!cancelled) {
          keep(false);
          setError('Recording recovery is unavailable in this browser.');
        }
      });
    const unload = (event: BeforeUnloadEvent) => {
      if (pending.current) {
        event.preventDefault();
        event.returnValue = '';
      }
    };
    const online = () => {
      if (draftRef.current) void upload();
    };
    window.addEventListener('online', online);
    window.addEventListener('beforeunload', unload);
    return () => {
      cancelled = true;
      alive.current = false;
      clearTimeout(timer.current);
      if (recorder.current?.state === 'recording') recorder.current.stop();
      stream.current?.getTracks().forEach((track) => track.stop());
      window.removeEventListener('beforeunload', unload);
      window.removeEventListener('online', online);
    };
  }, [session, question]);
  useEffect(() => {
    if (!draft) {
      setUrl('');
      return;
    }
    const next = URL.createObjectURL(draft.blob);
    setUrl(next);
    return () => URL.revokeObjectURL(next);
  }, [draft]);
  async function upload(value = draftRef.current) {
    if (!value || sending.current) return;
    sending.current = true;
    if (alive.current) {
      setBusy(true);
      setError('');
      setStatus('Uploading recording…');
    }
    const requestId = crypto.randomUUID();
    const finishTask = startTask('recording.upload', requestId);
    try {
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 90000);
      let response: Response;
      try {
        response = await loggedFetch(
          `/api/v1/candidate/questions/${value.question}/recordings/${value.id}`,
          {
            method: 'POST',
            headers: {
              Authorization: `Bearer ${token}`,
              'Content-Type': value.blob.type,
              'X-Request-ID': requestId,
            },
            body: value.blob,
            signal: controller.signal,
          },
        );
      } finally {
        clearTimeout(timeout);
      }
      if (!response.ok) {
        const body = await response.json().catch(() => null);
        throw new Error(
          body?.error ?? 'Recording was not submitted. Retry upload.',
        );
      }
      await removeRecording(value.key);
      draftRef.current = null;
      if (alive.current) {
        setDraft(null);
        keep(false);
        setStatus('Recording submitted. Your interviewer can play it.');
      }
      finishTask('succeeded');
    } catch (error) {
      finishTask('failed');
      if (alive.current) {
        setError(
          error instanceof Error
            ? error.message
            : 'Upload failed; recording kept on this device.',
        );
        setStatus('Recording not submitted · retry upload');
      }
    } finally {
      sending.current = false;
      if (alive.current) setBusy(false);
    }
  }
  async function start(video: boolean) {
    setBusy(true);
    setError('');
    keep(true);
    try {
      if (!window.MediaRecorder || !navigator.mediaDevices?.getUserMedia)
        throw new Error(
          'Recording is unavailable. Continue with a written answer.',
        );
      const captured = await navigator.mediaDevices.getUserMedia({
        audio: true,
        video,
      });
      if (!alive.current) {
        captured.getTracks().forEach((track) => track.stop());
        return;
      }
      stream.current = captured;
      const types = video
        ? ['video/webm;codecs=vp8,opus', 'video/webm', 'video/mp4']
        : ['audio/webm;codecs=opus', 'audio/ogg;codecs=opus', 'audio/mp4'];
      const mimeType = types.find((type) =>
        MediaRecorder.isTypeSupported(type),
      );
      if (!mimeType) {
        captured.getTracks().forEach((track) => track.stop());
        throw new Error(
          'No supported recording format. Continue with a written answer.',
        );
      }
      const current = new MediaRecorder(captured, {
        mimeType,
        videoBitsPerSecond: 600000,
        audioBitsPerSecond: 64000,
      });
      recorder.current = current;
      const id = crypto.randomUUID();
      const chunks: Blob[] = [];
      let size = 0;
      current.ondataavailable = (event) => {
        if (event.data.size) {
          chunks.push(event.data);
          size += event.data.size;
          if (size > 14 * 1024 * 1024 && current.state === 'recording')
            current.stop();
        }
      };
      current.onstop = () => {
        clearTimeout(timer.current);
        captured.getTracks().forEach((track) => track.stop());
        const blob = new Blob(chunks, { type: current.mimeType });
        const value = {
          key: recordingKey(session, question),
          session,
          question,
          id,
          blob,
        };
        draftRef.current = value;
        if (alive.current) {
          setRecording(false);
          setDraft(value);
          setStatus('Saving recording…');
        }
        void storeRecording(value)
          .then(() => upload(value))
          .catch(() => {
            if (alive.current) {
              setError(
                'Local recovery failed. Upload or download this recording before closing the tab.',
              );
              setBusy(false);
            }
          });
      };
      current.onerror = () => {
        if (alive.current)
          setError(
            'Recording was interrupted. Check the local preview before uploading.',
          );
      };
      current.start(1000);
      setRecording(true);
      setStatus('Recording · stops automatically after 60 seconds');
      timer.current = setTimeout(() => {
        if (current.state === 'recording') current.stop();
      }, 60000);
    } catch (error) {
      if (error instanceof DOMException && error.name === 'NotAllowedError')
        window.dispatchEvent(
          new CustomEvent('interview-device', { detail: 'recording_denied' }),
        );
      stream.current?.getTracks().forEach((track) => track.stop());
      if (alive.current) {
        keep(false);
        setError(
          error instanceof Error
            ? error.message
            : 'Unable to access camera/mic.',
        );
      }
    } finally {
      if (alive.current) setBusy(false);
    }
  }
  return (
    <details className="recorded-answer" open={!!draft || recording}>
      <summary>Optional audio/video answer</summary>
      <p className="muted">
        Record up to 60 seconds (15 MB). Stopping uploads the clip to your
        interviewer. Include a written answer for AI scoring; media alone
        requires human review.
      </p>
      <div className="actions">
        {!fullscreen && !draft && !recording && (
          <small>Enter fullscreen to start recording.</small>
        )}
        {recording ? (
          <Button
            type="button"
            variant="standard"
            onClick={() => recorder.current?.stop()}
          >
            Stop & upload
          </Button>
        ) : (
          !draft && (
            <>
              <Button
                type="button"
                variant="secondary"
                disabled={busy || !fullscreen}
                onClick={() => void start(false)}
              >
                Record audio
              </Button>
              <Button
                type="button"
                variant="secondary"
                disabled={busy || !fullscreen}
                onClick={() => void start(true)}
              >
                Record video
              </Button>
            </>
          )
        )}
        {draft && (
          <>
            <Button
              type="button"
              variant="standard"
              disabled={busy}
              onClick={() => void upload()}
            >
              Retry upload
            </Button>
            <a
              className="button secondary"
              href={url}
              download={`answer-${draft.id}.${draft.blob.type.includes('mp4') ? 'mp4' : draft.blob.type.includes('ogg') ? 'ogg' : 'webm'}`}
            >
              Download local copy
            </a>
            <Button
              type="button"
              variant="text"
              disabled={busy}
              onClick={() =>
                void removeRecording(draft.key)
                  .then(() => {
                    draftRef.current = null;
                    setDraft(null);
                    keep(false);
                    setStatus('Local recording discarded.');
                  })
                  .catch(() =>
                    setError('Unable to discard the local recording.'),
                  )
              }
            >
              Discard local recording
            </Button>
          </>
        )}
      </div>
      {draft &&
        (draft.blob.type.startsWith('video/') ? (
          <video controls src={url} />
        ) : (
          <audio controls src={url} />
        ))}
      {status && <p role="status">{status}</p>}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </details>
  );
}
