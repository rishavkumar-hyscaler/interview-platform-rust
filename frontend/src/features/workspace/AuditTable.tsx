import type { Audit } from './api';
import { auditPresentation } from './auditPresentation';

export function AuditTable({
  events,
  compact = false,
}: {
  events: Audit[];
  compact?: boolean;
}) {
  if (!events.length)
    return (
      <div className="empty compact">No audit events match these filters.</div>
    );
  return (
    <div className="table-scroll">
      <table className={compact ? 'min-w-[30rem]' : 'min-w-[44rem]'}>
        <thead>
          <tr>
            <th scope="col">Event</th>
            <th scope="col">Performed by</th>
            {!compact && (
              <>
                <th scope="col">Job ID</th>
                <th scope="col">Job title</th>
                <th scope="col">Candidate</th>
              </>
            )}
            <th scope="col">Timestamp</th>
          </tr>
        </thead>
        <tbody>
          {events.map((event) => {
            const { title, performedBy, details } = auditPresentation(event);
            const timestamp = new Date(event.created_at);
            return (
              <tr key={event.id} className="align-top">
                <td>
                  <strong className="font-normal!">{title}</strong>
                  {details.length > 0 && <small>{details.join(' · ')}</small>}
                </td>
                <td>{performedBy}</td>
                {!compact && (
                  <>
                    <td className="whitespace-nowrap">
                      {event.job_code || '—'}
                    </td>
                    <td>{event.job_title || '—'}</td>
                    <td>
                      {event.candidate_name || event.candidate_email || '—'}
                      {event.candidate_name && event.candidate_email && (
                        <small>{event.candidate_email}</small>
                      )}
                    </td>
                  </>
                )}
                <td className="whitespace-nowrap tabular-nums">
                  <time dateTime={event.created_at}>
                    {timestamp.toLocaleTimeString()}
                    <small>
                      {timestamp.toLocaleDateString(undefined, {
                        dateStyle: 'medium',
                      })}
                    </small>
                  </time>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
