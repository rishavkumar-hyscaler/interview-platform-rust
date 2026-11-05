import { useEffect, useState } from 'react';
import { pendingRecordings, type RecordingDraft } from './recordingStore';
export function UnsubmittedRecordings({ session }: { session: string }) {
  const [drafts, setDrafts] = useState<RecordingDraft[]>([]); const [links, setLinks] = useState<{ id: string; url: string; extension: string }[]>([]);
  useEffect(() => { let active = true; const refresh = () => void pendingRecordings(session).then(values => { if (active) setDrafts(previous => previous.length === values.length && previous.every((item, index) => item.id === values[index].id) ? previous : values); }).catch(() => {}); refresh(); const timer = setInterval(refresh, 3000); return () => { active = false; clearInterval(timer); }; }, [session]);
  useEffect(() => { const next = drafts.map(draft => ({ id: draft.id, url: URL.createObjectURL(draft.blob), extension: draft.blob.type.includes('mp4') ? 'mp4' : draft.blob.type.includes('ogg') ? 'ogg' : 'webm' })); setLinks(next); return () => next.forEach(link => URL.revokeObjectURL(link.url)); }, [drafts]);
  if (!drafts.length) return null;
  return <section className="configuration-note"><h3>Recordings that may not have reached the server</h3><p>These local copies are not confirmed as submitted. Download them for your records; contact your interviewer if needed.</p>{links.map(link => <p key={link.id}><a href={link.url} download={`unsubmitted-${link.id}.${link.extension}`}>Download local recording</a></p>)}</section>;
}
