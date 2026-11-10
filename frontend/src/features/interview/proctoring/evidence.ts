import { candidateApi } from '../api';
import { ApiError } from '../../../api';
import { loggedFetch } from '../../../transport';
import { startTask } from '../../../observability';
import type { MediaObservation } from './signals';
type Asset = {
  id: string;
  blob: Blob;
  source?: 'camera' | 'screen';
  surface?: string;
  captured?: number;
  phase?: 'signal_window' | 'post_signal';
};
type Pending = {
  id: string;
  session: string;
  observation: MediaObservation;
  assets: Asset[];
  created: number;
  accepted: boolean;
};
function store<T>(
  mode: IDBTransactionMode,
  action: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  return new Promise((resolve, reject) => {
    const opening = indexedDB.open('interview-proctor-evidence', 1);
    opening.onupgradeneeded = () =>
      opening.result.createObjectStore('pending', { keyPath: 'id' });
    opening.onerror = () => reject(new Error('Evidence recovery unavailable'));
    opening.onsuccess = () => {
      const db = opening.result;
      const tx = db.transaction('pending', mode);
      const result = action(tx.objectStore('pending'));
      tx.oncomplete = () => {
        db.close();
        resolve(result.result);
      };
      tx.onerror = tx.onabort = () => {
        db.close();
        reject(new Error('Evidence recovery unavailable'));
      };
    };
  });
}
// Camera and screen queues share IndexedDB; only one may drain a session at a time.
const flushingSessions = new Set<string>();
export function evidenceQueue(
  token: string,
  session: string,
  status: (message: string) => void,
) {
  let sending = false;
  let stopped = false;
  const acknowledged = new Set<string>();
  const notice = (message: string) => {
    if (!stopped) status(message);
  };
  async function flush() {
    if (stopped || sending || flushingSessions.has(session)) return;
    sending = true;
    flushingSessions.add(session);
    try {
      const records = await store<Pending[]>('readonly', (value) =>
        value.getAll(),
      );
      let failed = 0;
      for (const record of records) {
        if (stopped) break;
        try {
          if (Date.now() - record.created > 14 * 60000) {
            await store('readwrite', (value) => value.delete(record.id));
            notice(
              'Some pending evidence expired before upload. Browser events remain separate.',
            );
            continue;
          }
          if (record.session !== session) continue;
          // Older clients stored fractional performance.now() durations, which the
          // API correctly rejects for its integer millisecond field. Repair queued,
          // unaccepted events without changing their IDs or original capture times.
          if (!record.accepted && record.observation.measurements) {
            record.observation.measurements.duration_ms = Math.floor(
              record.observation.measurements.duration_ms,
            );
          }
          if (!record.accepted && !acknowledged.has(record.observation.id)) {
            try {
              await candidateApi(token).events([record.observation]);
            } catch (error) {
              // A submitted session rejects event replays. The attachment endpoint still
              // verifies the previously received event before accepting any late media.
              if (!(
                error instanceof ApiError &&
                error.status === 409 &&
                record.assets.length
              ))
                throw error;
            }
          }
          acknowledged.add(record.observation.id);
          record.accepted = true;
          await store('readwrite', (value) => value.put(record));
          for (const asset of record.assets) {
            const finish = startTask('proctor.evidence.upload');
            try {
              const response = await loggedFetch(
                `/api/v1/candidate/events/${record.observation.id}/evidence/${asset.id}`,
                {
                  method: 'POST',
                  headers: {
                    Authorization: `Bearer ${token}`,
                    'Content-Type': asset.blob.type,
                    'X-Evidence-Source': asset.source ?? 'camera',
                    ...(asset.phase
                      ? { 'X-Evidence-Capture': asset.phase }
                      : {}),
                    ...(asset.surface
                      ? { 'X-Capture-Surface': asset.surface }
                      : {}),
                    ...(asset.captured
                      ? { 'X-Evidence-Captured-At': String(asset.captured) }
                      : {}),
                  },
                  body: asset.blob,
                  signal: AbortSignal.timeout(30000),
                },
              );
              if (!response.ok)
                throw new ApiError('Evidence upload rejected', response.status);
              finish('succeeded');
              // Checkpoint each asset so later failures do not replay successful uploads.
              record.assets = record.assets.filter(
                (pending) => pending.id !== asset.id,
              );
              await store('readwrite', (value) => value.put(record));
            } catch (error) {
              finish('failed');
              throw error;
            }
          }
          await store('readwrite', (value) => value.delete(record.id));
        } catch (error) {
          failed++;
          notice(
            error instanceof ApiError
              ? `Media evidence pending · server returned HTTP ${error.status}: ${error.message}`
              : 'Media evidence pending · reconnect to retry (up to 14 minutes)',
          );
          // Keep this record for retry; a failed upload must not block later flags.
        }
      }
      if (!failed && !stopped) notice('Media observations and evidence synced');
    } catch (error) {
      notice(
        error instanceof ApiError
          ? `Media evidence pending · server returned HTTP ${error.status}: ${error.message}`
          : 'Media evidence pending · reconnect to retry (up to 14 minutes)',
      );
    } finally {
      sending = false;
      flushingSessions.delete(session);
    }
  }
  const timer = setInterval(() => void flush(), 5000);
  window.addEventListener('online', flush);
  void flush();
  return {
    async enqueue(observation: MediaObservation, assets: Asset[]) {
      let records: Pending[];
      try {
        records = await store<Pending[]>('readonly', (value) => value.getAll());
      } catch {
        // Preserve the observation even when IndexedDB is blocked or unavailable.
        await candidateApi(token).events([observation]);
        notice('Observation sent · local evidence recovery unavailable');
        if (assets.length)
          throw new Error(
            'Local evidence storage unavailable; attachment was not saved',
          );
        return;
      }
      if (records.filter((record) => record.session === session).length >= 20) {
        notice(
          'Media evidence buffer full · observation sent, attachment could not be buffered',
        );
        await candidateApi(token).events([observation]);
        if (assets.length)
          throw new Error('Evidence buffer full; attachment was not saved');
        return;
      }
      await store('readwrite', (value) =>
        value.put({
          id: crypto.randomUUID(),
          session,
          observation,
          assets,
          created: Date.now(),
          accepted: acknowledged.has(observation.id),
        } satisfies Pending),
      );
      void flush();
    },
    stop() {
      stopped = true;
      clearInterval(timer);
      window.removeEventListener('online', flush);
    },
  };
}
export async function snapshot(
  video: HTMLVideoElement,
  maxWidth = 640,
): Promise<Blob | null> {
  if (video.readyState < 2 || !video.videoWidth) return null;
  const canvas = document.createElement('canvas');
  const scale = Math.min(
    1,
    maxWidth / video.videoWidth,
    900 / video.videoHeight,
  );
  canvas.width = Math.round(video.videoWidth * scale);
  canvas.height = Math.round(video.videoHeight * scale);
  const context = canvas.getContext('2d');
  if (!context) return null;
  context.drawImage(video, 0, 0, canvas.width, canvas.height);
  return new Promise((resolve) =>
    canvas.toBlob(
      (blob) => resolve(blob && blob.size <= 512 * 1024 ? blob : null),
      'image/jpeg',
      0.65,
    ),
  );
}
export function shortClip(
  stream: MediaStream,
  register: (stop: () => void) => void,
): Promise<Blob | null> {
  if (typeof MediaRecorder === 'undefined') return Promise.resolve(null);
  const video = stream
    .getVideoTracks()
    .some((track) => track.readyState === 'live');
  const format = (
    video
      ? ['video/webm;codecs=vp8,opus', 'video/webm', 'video/mp4']
      : ['audio/webm;codecs=opus', 'audio/webm', 'audio/ogg', 'audio/mp4']
  ).find((type) => MediaRecorder.isTypeSupported(type));
  if (!format) return Promise.resolve(null);
  return new Promise((resolve) => {
    const clips: Blob[] = [];
    let size = 0;
    const recorder = new MediaRecorder(stream, {
      mimeType: format,
      videoBitsPerSecond: 350000,
      audioBitsPerSecond: 32000,
    });
    const stop = () => {
      if (recorder.state !== 'inactive') recorder.stop();
    };
    register(stop);
    recorder.ondataavailable = (event) => {
      clips.push(event.data);
      size += event.data.size;
      if (size > 1900000) stop();
    };
    recorder.onstop = () => {
      clearTimeout(timer);
      const blob = new Blob(clips, { type: recorder.mimeType });
      resolve(blob.size >= 16 && blob.size <= 2 * 1024 * 1024 ? blob : null);
    };
    recorder.onerror = () => {
      clearTimeout(timer);
      resolve(null);
    };
    recorder.start(250);
    const timer = setTimeout(stop, 3000);
  });
}
