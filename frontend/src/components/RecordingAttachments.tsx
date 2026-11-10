import { Button } from './ui/button';
import { HelpTip } from '../features/workspace/HelpTip';
import { loggedFetch } from '../transport';
import { useEffect, useState } from 'react';
import { request } from '../api';
type Recording = { recording_id: string; question_id: string; mime: string; bytes: number; saved_at: string };
function Playback({ recording, session, token, candidate }: { recording: Recording; session: string; token: string; candidate: boolean }) {
  const [url, setUrl] = useState(''); const [error, setError] = useState(''); const [busy, setBusy] = useState(false);
  useEffect(() => () => { if (url) URL.revokeObjectURL(url); }, [url]);
  async function load() {
    setBusy(true); setError('');
    try { const response = await loggedFetch(`${candidate ? '/api/v1/candidate' : `/api/v1/sessions/${session}`}/recordings/${recording.recording_id}`, { headers: { Authorization: `Bearer ${token}` } }); if (!response.ok) throw new Error('Recording unavailable or access expired.'); setUrl(URL.createObjectURL(await response.blob())); }
    catch (error) { setError(error instanceof Error ? error.message : 'Unable to play recording'); }
    finally { setBusy(false); }
  }
  return <article className="recording-evidence"><p><strong>{recording.mime.startsWith('video/') ? 'Video' : 'Audio'} answer</strong> · {new Date(recording.saved_at).toLocaleString()} · {(recording.bytes / 1048576).toFixed(1)} MB</p><small>Question ID: {recording.question_id}</small>{url ? recording.mime.startsWith('video/') ? <video controls src={url} /> : <audio controls src={url} /> : <Button variant="secondary" disabled={busy} onClick={() => void load()}>{busy ? 'Loading…' : 'Load private recording'}</Button>}{error && <p role="alert" className="error">{error}</p>}</article>;
}
export function RecordingAttachments({ session, token, question, candidate = false, refresh = 0, onPresence }: { onPresence?: (present: boolean) => void; session: string; token: string; question?: string; candidate?: boolean; refresh?: number }) {
  const [items, setItems] = useState<Recording[]>([]); const [error, setError] = useState('');
  useEffect(() => { const controller = new AbortController(); void request<{ recordings: Recording[] }>(`${candidate ? '/api/v1/candidate' : `/api/v1/sessions/${session}`}/recordings`, { signal: controller.signal }, token).then(value => { const items = value.recordings.filter(item => !question || item.question_id === question); setItems(items); onPresence?.(items.length > 0); }).catch(() => { if (!controller.signal.aborted) setError('Unable to load recordings. Refresh to retry.'); }); return () => controller.abort(); }, [session, token, question, candidate, refresh]);
  if (candidate && !items.length && !error) return null;
  return <section className={candidate ? 'recording-attachments' : 'question-preparation'}><h3 className={candidate ? undefined : "flex items-center justify-between gap-2"}>Recorded answers {!candidate && <HelpTip text="Media supplements the written answer. Media-only responses require human review and are not assigned a zero AI score." />}</h3>{items.map(item => <Playback key={item.recording_id} recording={item} session={session} token={token} candidate={candidate} />)}{!items.length && !error && <p>No recordings uploaded.</p>}{error && <p role="alert" className="error">{error}</p>}</section>;
}
