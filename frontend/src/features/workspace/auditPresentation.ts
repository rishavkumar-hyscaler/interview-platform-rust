import type { Audit } from './api';

const titles: Record<string, string> = {
  'job.blueprint_task': 'Job blueprint preparation',
  'job.created': 'Job created',
  'session.invited': 'Candidate invited',
  'session.accepted': 'Invitation accepted',
  'session.started': 'Interview started',
  'session.in_progress': 'Interview started',
  'session.ready': 'Candidate ready for interview',
  'session.submitted': 'Interview submitted',
  'session.expired': 'Interview time expired',
  'answer.saved': 'Answer saved',
  'question.prepared': 'Question prepared',
  'questions.prepared': 'Question set ready',
  'questions.queued': 'Question preparation queued',
  'generation.rejected': 'Question needs revision',
  'assessment.started': 'AI scoring started',
  'assessment.question.scored': 'Question scored',
  'assessment.completed': 'AI scoring completed',
  'assessment.failed': 'AI scoring failed',
  'adaptive.selected': 'Next question selected',
  'question.served': 'Question opened',
  'proctor.evidence': 'Evidence attachment saved',
  'proctor.observed': 'Browser activity recorded',
  'invitation.preparation_queued': 'Candidate question preparation queued',
  'generation.retried': 'Candidate question preparation retried',
  'invitation.link_retrieved': 'Invitation link retrieved',
  'reviewer.message': 'Interviewer asked the assistant',
  'rag.completed': 'Interview evidence ready to search',
};

function number(value: unknown): number | null {
  return typeof value === 'number' && Number.isInteger(value) && value > 0
    ? value
    : null;
}

export function auditPresentation(event: Audit) {
  const actor = String(event.metadata.actor ?? '').toLowerCase();
  const candidateAction = ['candidate', 'interviewee'].includes(actor);
  const performedBy = event.actor_id
    ? `Interviewer${event.actor_email ? ` · ${event.actor_email}` : ''}`
    : candidateAction
      ? `Candidate${event.candidate_name ? ` · ${event.candidate_name}` : ''}`
      : actor === 'worker'
        ? 'Background worker'
        : 'System (automatic)';
  const rawTitle = event.action.replaceAll('.', ' ').replaceAll('_', ' ');
  const title =
    titles[event.action] ??
    rawTitle.charAt(0).toUpperCase() + rawTitle.slice(1);
  const result =
    event.metadata.result && typeof event.metadata.result === 'object'
      ? (event.metadata.result as Record<string, unknown>)
      : {};
  const position =
    event.question_position ??
    number(event.metadata.position) ??
    number(event.metadata.question_position) ??
    number(result.position);
  const details: string[] = [];
  if (position) details.push(`Question ${position}`);
  if (
    event.action === 'adaptive.selected' &&
    number(event.metadata.next_position)
  )
    details.push(`Next: question ${event.metadata.next_position}`);
  if (event.action === 'question.prepared' && number(event.metadata.total))
    details.push(`${event.metadata.total} questions planned`);
  if (event.action === 'questions.prepared' && number(event.metadata.count))
    details.push(`${event.metadata.count} questions ready`);
  if (event.action === 'answer.saved' && number(event.metadata.revision))
    details.push(`Saved revision ${event.metadata.revision}`);
  if (
    event.action === 'assessment.question.scored' &&
    typeof result.score === 'number' &&
    typeof result.max_score === 'number'
  ) {
    details.push(`Provisional score: ${result.score}/${result.max_score}`);
  }
  if (event.action === 'proctor.observed') {
    if (typeof event.metadata.kind === 'string')
      details.push(event.metadata.kind.replaceAll('_', ' '));
    details.push('Observation only; requires human review');
  }
  return { title, performedBy, details };
}
