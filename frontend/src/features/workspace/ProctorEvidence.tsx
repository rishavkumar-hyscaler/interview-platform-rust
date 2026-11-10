import { Button } from '../../components/ui/button';
import { ExpandableSection } from './ExpandableSection';
import { useEffect, useState } from 'react';
import { loggedFetch } from '../../transport';
export type ProctorAttachment = {
  asset_id: string;
  mime: string;
  bytes: number;
  saved_at: string;
  evidence_source?: string;
  capture_surface?: string;
  capture?: string;
  captured_at_ms?: number | null;
};
function Attachment({
  attachment,
  session,
  token,
}: {
  attachment: ProctorAttachment;
  session: string;
  token: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const [url, setUrl] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(
    () => () => {
      if (url) URL.revokeObjectURL(url);
    },
    [url],
  );
  async function load() {
    setBusy(true);
    setError('');
    try {
      const response = await loggedFetch(
        `/api/v1/sessions/${session}/evidence/${attachment.asset_id}`,
        {
          headers: { Authorization: `Bearer ${token}` },
          signal: AbortSignal.timeout(30000),
        },
      );
      if (!response.ok)
        throw new Error(
          'Evidence unavailable or integrity verification failed',
        );
      setUrl(URL.createObjectURL(await response.blob()));
      setExpanded(true);
    } catch {
      setError('Unable to load this private evidence attachment.');
    } finally {
      setBusy(false);
    }
  }
  const label = attachment.mime.startsWith('image/')
    ? attachment.evidence_source === 'screen'
      ? 'Screen screenshot'
      : 'Camera snapshot'
    : attachment.mime.startsWith('video/')
      ? 'Video clip'
      : 'Audio clip';
  return (
    <article className="recording-evidence proctor-attachment">
      <p>
        <strong>{label}</strong> ·{' '}
        {new Date(attachment.saved_at).toLocaleString()} ·{' '}
        {Math.ceil(attachment.bytes / 1024)} KB
        {attachment.captured_at_ms != null &&
          ` · captured ${new Date(attachment.captured_at_ms).toLocaleString()}`}
        {attachment.capture === 'signal_window' &&
          ' · speech window leading up to flag'}
        {attachment.evidence_source === 'screen' &&
          ` · ${attachment.capture_surface ?? 'unknown'} surface`}
      </p>
      {!url ? (
        <Button variant="secondary" disabled={busy} onClick={() => void load()}>
          {busy ? 'Verifying evidence…' : `Load private ${label.toLowerCase()}`}
        </Button>
      ) : null}
      {url && (
        <ExpandableSection
          label={label}
          open={expanded}
          onOpenChange={setExpanded}
        >
          <div className="proctor-media">
            {attachment.mime.startsWith('image/') ? (
              <img
                src={url}
                alt={
                  attachment.evidence_source === 'screen'
                    ? 'Shared-screen screenshot captured near this browser event'
                    : 'Camera snapshot captured after this proctoring signal'
                }
                style={{ maxWidth: '100%' }}
              />
            ) : attachment.mime.startsWith('video/') ? (
              <video src={url} controls preload="metadata" />
            ) : (
              <audio src={url} controls preload="metadata" />
            )}
          </div>
        </ExpandableSection>
      )}
      {error && <p role="alert">{error}</p>}
    </article>
  );
}
function DecisionBadge({ decision }: { decision?: string }) {
  const status =
    decision === 'dismissed'
      ? {
          label: 'Dismissed',
          color:
            'bg-emerald-100 text-emerald-800 workspace-dark:bg-emerald-950 workspace-dark:text-emerald-300',
        }
      : decision === 'confirmed_activity'
        ? {
            label: 'Activity confirmed',
            color:
              'bg-red-100 text-red-800 workspace-dark:bg-red-950 workspace-dark:text-red-300',
          }
        : decision === 'suspected'
          ? {
              label: 'Needs further review',
              color:
                'bg-amber-100 text-amber-900 workspace-dark:bg-amber-950 workspace-dark:text-amber-300',
            }
          : {
              label: 'Unreviewed',
              color:
                'bg-slate-100 text-slate-700 workspace-dark:bg-slate-800 workspace-dark:text-slate-300',
            };

  return (
    <span
      className={`rounded-full px-2 py-0.5 text-detail font-medium ${status.color}`}
    >
      {status.label}
    </span>
  );
}

export function ProctorEvidence({
  label,
  events,
  session,
  token,
  showDecisions = false,
}: {
  label: string;
  showDecisions?: boolean;
  events: {
    id: string;
    received_at: string;
    chain_status?: string;
    evidence: { kind: string; measurements?: Record<string, number> };
    attachments?: ProctorAttachment[];
    annotation?: { decision: string } | null;
  }[];
  session: string;
  token: string;
}) {
  if (!events.length) return null;
  return (
    <ExpandableSection
      className="proctor-evidence"
      label={`${label} (${events.length})`}
      help="Screen screenshots show only the surface the candidate chose to share, near the event time. Browser-submitted images are not proof of every activity or an automatic cheating verdict. Eye/head direction is a calibrated geometric estimate. Speech with no visible speaker, little mouth motion, or multiple faces may come from the candidate, another person, or a recording. These observations do not establish attention, intent or cheating, and do not affect answer scores."
    >
      {events.map((event) => (
        <ExpandableSection
          className="proctor-event"
          key={event.id}
          label={
            <span className="flex flex-wrap items-center gap-2">
              {event.evidence.kind.replaceAll('_', ' ')} ·{' '}
              {new Date(event.received_at).toLocaleString()}
              {event.attachments?.length
                ? ` · ${event.attachments.length} attachments`
                : ''}
              {showDecisions && (
                <DecisionBadge decision={event.annotation?.decision} />
              )}
            </span>
          }
        >
          <ExpandableSection
            className="proctor-technical"
            label={<>Technical details</>}
          >
            <small>Receipt chain: {event.chain_status ?? 'unknown'}</small>
            <pre>
              {JSON.stringify(event.evidence.measurements ?? {}, null, 2)}
            </pre>
          </ExpandableSection>
          {event.attachments?.map((attachment) => (
            <Attachment
              key={attachment.asset_id}
              attachment={attachment}
              session={session}
              token={token}
            />
          ))}
          {!event.attachments?.length && (
            <p>
              {event.evidence.kind === 'model_unavailable'
                ? 'Monitoring was unavailable. This is a coverage gap, not evidence of candidate misconduct.'
                : 'No media attached to this event.'}
            </p>
          )}
        </ExpandableSection>
      ))}
    </ExpandableSection>
  );
}
