import { useEffect, useState } from 'react';
import { loggedFetch } from '../../transport';
export type ProctorAttachment = { asset_id: string; mime: string; bytes: number; saved_at: string; evidence_source?: string; capture_surface?: string; captured_at_ms?: number | null };
function Attachment({ attachment, session, token }: { attachment: ProctorAttachment; session: string; token: string }) {
  const [url, setUrl] = useState(''); const [error, setError] = useState(''); const [busy, setBusy] = useState(false);
  useEffect(() => () => { if (url) URL.revokeObjectURL(url); }, [url]);
  async function load() {
    setBusy(true); setError('');
    try {
      const response = await loggedFetch(`/api/v1/sessions/${session}/evidence/${attachment.asset_id}`, { headers: { Authorization: `Bearer ${token}` }, signal: AbortSignal.timeout(30000) });
      if (!response.ok) throw new Error('Evidence unavailable or integrity verification failed');
      setUrl(URL.createObjectURL(await response.blob()));
    } catch { setError('Unable to load this private evidence attachment.'); }
    finally { setBusy(false); }
  }
  const label = attachment.mime.startsWith('image/') ? attachment.evidence_source === 'screen' ? 'Screen screenshot' : 'Camera snapshot' : attachment.mime.startsWith('video/') ? 'Video clip' : 'Audio clip';
  return <article className="recording-evidence"><p><strong>{label}</strong> · {new Date(attachment.saved_at).toLocaleString()} · {Math.ceil(attachment.bytes / 1024)} KB{attachment.evidence_source === 'screen' && ` · ${attachment.capture_surface ?? 'unknown'} surface`}</p>{!url ? <button className="button secondary" disabled={busy} onClick={() => void load()}>{busy ? 'Verifying evidence…' : `Load private ${label.toLowerCase()}`}</button> : attachment.mime.startsWith('image/') ? <img src={url} alt={attachment.evidence_source === 'screen' ? 'Shared-screen screenshot captured near this browser event' : 'Camera snapshot captured after this proctoring signal'} style={{ maxWidth: '100%' }} /> : attachment.mime.startsWith('video/') ? <video src={url} controls /> : <audio src={url} controls />}{error && <p role="alert">{error}</p>}</article>;
}
export function ProctorEvidence({ events, session, token }: { events: { id: string; received_at: string; chain_status?: string; evidence: { kind: string; measurements?: Record<string, number> }; attachments?: ProctorAttachment[] }[]; session: string; token: string }) {
  const media = events.filter(event => event.attachments?.length || ['face_missing', 'multiple_faces', 'gaze_away', 'possible_background_speech', 'speech_detected'].includes(event.evidence.kind));
  if (!media.length) return null;
  return <details><summary>Screen and camera/audio evidence ({media.length})</summary><p className="muted">Screen screenshots show only the surface the candidate chose to share, near the event time. Browser-submitted images are not proof of every activity or an automatic cheating verdict. Eye/head direction is a calibrated geometric estimate. Speech without visible mouth motion may come from the candidate, another person, or a recording. These observations do not establish attention, intent or cheating, and do not affect answer scores.</p>{media.map(event => <details key={event.id}><summary>{event.evidence.kind.replaceAll('_', ' ')} · {new Date(event.received_at).toLocaleString()}</summary><small>Receipt chain: {event.chain_status ?? 'unknown'}</small><pre>{JSON.stringify(event.evidence.measurements ?? {}, null, 2)}</pre>{event.attachments?.map(attachment => <Attachment key={attachment.asset_id} attachment={attachment} session={session} token={token} />)}{!event.attachments?.length && <p>No media attached (capture disabled, unavailable, throttled, pending, or expired). The observation alone is unverified.</p>}</details>)}</details>;
}
