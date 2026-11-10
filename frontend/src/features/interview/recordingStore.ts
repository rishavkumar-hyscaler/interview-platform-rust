export type RecordingDraft = {
  key: string;
  session: string;
  question: string;
  id: string;
  blob: Blob;
};
function database(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('interview-recordings', 1);
    request.onupgradeneeded = () =>
      request.result.createObjectStore('drafts', { keyPath: 'key' });
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
async function transaction<T>(
  mode: IDBTransactionMode,
  action: (store: IDBObjectStore) => IDBRequest<T>,
): Promise<T> {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction('drafts', mode);
    const result = action(tx.objectStore('drafts'));
    tx.oncomplete = () => {
      db.close();
      resolve(result.result);
    };
    tx.onerror = tx.onabort = () => {
      db.close();
      reject(tx.error ?? new Error('Recording recovery unavailable'));
    };
  });
}
export const recordingKey = (session: string, question: string) =>
  `${session}:${question}`;
export const storeRecording = (draft: RecordingDraft) =>
  transaction('readwrite', (store) => store.put(draft));
export const readRecording = (session: string, question: string) =>
  transaction<RecordingDraft | undefined>('readonly', (store) =>
    store.get(recordingKey(session, question)),
  );
export const removeRecording = (key: string) =>
  transaction('readwrite', (store) => store.delete(key));
export const pendingRecordings = async (session: string) =>
  (
    await transaction<RecordingDraft[]>('readonly', (store) => store.getAll())
  ).filter((draft) => draft.session === session);
