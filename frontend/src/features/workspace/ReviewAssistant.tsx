import { Button } from '../../components/ui/button';
import { Select } from '../../components/ui/select';
import { RefreshButton } from '../../components/ui/refresh-button';
import { ExpandableSection } from './ExpandableSection';
import { HelpTip } from './HelpTip';
import { EvidenceIndex } from './EvidenceIndex';
import { ProctorEvidence, type ProctorAttachment } from './ProctorEvidence';
import { startTask } from '../../observability';
import { loggedFetch } from '../../transport';
import { useEffect, useRef, useState } from 'react';
import { request } from '../../api';
type Reply = {
  retrieval_mode?: string;
  answer: string;
  citations: { source_id: string; quote: string }[];
  sources: { id: string; text: string }[];
  next_page: number | null;
};
type Integrity = {
  risk_score: number;
  summary: string;
  notice: string;
  events: {
    id: string;
    received_at: string;
    evidence: { kind: string; measurements?: Record<string, number> };
    attachments?: ProctorAttachment[];
    chain_status?: string;
    annotation: null | { decision: string; note: string };
  }[];
};
type LiveEvent = {
  id: string;
  action: string;
  created_at: string;
  metadata: Record<string, unknown>;
};
const integritySignalKinds = new Set([
  'fullscreen_exited',
  'focus_lost',
  'hidden',
  'multiple_displays',
  'devtools_suspected',
  'paste',
  'context_menu',
  'typing_burst',
  'rapid_answer',
  'camera_disabled',
  'microphone_disabled',
  'screen_share_stopped',
  'face_missing',
  'multiple_faces',
  'gaze_away',
  'possible_background_speech',
]);

export function ReviewAssistant({
  session,
  token,
}: {
  session: string;
  token: string;
}) {
  const [refreshing, setRefreshing] = useState(false);
  const [question, setQuestion] = useState('');
  const [replies, setReplies] = useState<Reply[]>([]);
  const [busy, setBusy] = useState(false);
  const [draft, setDraft] = useState('');
  const [error, setError] = useState('');
  const [integrity, setIntegrity] = useState<Integrity | null>(null);
  const [events, setEvents] = useState<LiveEvent[]>([]);
  const [connection, setConnection] = useState('Connecting to live session…');
  const [selected, setSelected] = useState('');
  const [decision, setDecision] = useState('dismissed');
  const [note, setNote] = useState('');
  const conversation = useRef(crypto.randomUUID());
  const abort = useRef<AbortController | null>(null);
  const integrityRequest = useRef(0);
  async function refreshIntegrity(signal?: AbortSignal) {
    const version = ++integrityRequest.current;
    const result = await request<Integrity>(
      `/api/v1/sessions/${session}/integrity`,
      { signal },
      token,
    );
    // A slower response for an earlier observation must not hide newer attachments.
    if (version === integrityRequest.current && !signal?.aborted)
      setIntegrity(result);
  }
  useEffect(() => {
    const controller = new AbortController();
    let current: AbortController | null = null;
    let timer: ReturnType<typeof setTimeout>;
    let cursor = '';
    void refreshIntegrity(controller.signal).catch(() => {});
    async function connect() {
      current = new AbortController();
      const requestId = crypto.randomUUID();
      const finishTask = startTask('review.live', requestId);
      try {
        const response = await loggedFetch(
          `/api/v1/sessions/${session}/live${cursor ? `?after=${cursor}` : ''}`,
          {
            headers: {
              Authorization: `Bearer ${token}`,
              'X-Request-ID': requestId,
            },
            signal: current.signal,
          },
        );
        if (!response.ok || !response.body)
          throw new Error('Live connection unavailable');
        setConnection('Live session connected');
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';
        while (!controller.signal.aborted) {
          const { value, done } = await reader.read();
          if (done) break;
          buffer += decoder
            .decode(value, { stream: true })
            .replaceAll('\r\n', '\n');
          let end: number;
          while ((end = buffer.indexOf('\n\n')) >= 0) {
            const block = buffer.slice(0, end);
            buffer = buffer.slice(end + 2);
            const data = block
              .split('\n')
              .filter((line) => line.startsWith('data:'))
              .map((line) => line.slice(5).trimStart())
              .join('\n');
            if (!data) continue;
            const event: LiveEvent = JSON.parse(data);
            cursor = event.id;
            setEvents((previous) =>
              previous.some((item) => item.id === event.id)
                ? previous
                : [...previous, event].slice(-30),
            );
            if (
              event.action === 'proctor.observed' ||
              event.action === 'proctor.evidence' ||
              event.action === 'integrity.annotated'
            )
              void refreshIntegrity(controller.signal).catch(() => {});
          }
        }
        finishTask(controller.signal.aborted ? 'cancelled' : 'succeeded');
      } catch {
        finishTask(controller.signal.aborted ? 'cancelled' : 'failed');
      }
      if (!controller.signal.aborted) {
        setConnection('Live connection interrupted · reconnecting');
        timer = setTimeout(() => void connect(), 3000);
      }
    }
    void connect();
    return () => {
      controller.abort();
      current?.abort();
      clearTimeout(timer);
      abort.current?.abort();
    };
  }, [session, token]);
  async function ask(page = 0, prompt = question) {
    if (!prompt.trim()) return;
    const requestId = crypto.randomUUID();
    const finishTask = startTask('review.chat', requestId);
    setBusy(true);
    setError('');
    const controller = new AbortController();
    abort.current = controller;
    try {
      setDraft('');
      const response = await loggedFetch(`/api/v1/sessions/${session}/review`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${token}`,
          'X-Request-ID': requestId,
        },
        signal: controller.signal,
        body: JSON.stringify({
          question: prompt,
          conversation: conversation.current,
          page,
        }),
      });
      if (!response.ok || !response.body)
        throw new Error(
          'Reviewer service unavailable. Retry after checking the backend.',
        );
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';
      let raw = '';
      let completed = false;
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder
          .decode(value, { stream: true })
          .replaceAll('\r\n', '\n');
        let end: number;
        while ((end = buffer.indexOf('\n\n')) >= 0) {
          const block = buffer.slice(0, end);
          buffer = buffer.slice(end + 2);
          const kind = block
            .split('\n')
            .find((line) => line.startsWith('event:'))
            ?.slice(6)
            .trim();
          const data = block
            .split('\n')
            .filter((line) => line.startsWith('data:'))
            .map((line) => line.slice(5).trimStart())
            .join('\n');
          if (!data) continue;
          if (kind === 'failure') {
            setDraft('');
            throw new Error(JSON.parse(data).error);
          }
          if (kind === 'delta') {
            raw += JSON.parse(data).delta;
            const match = raw.match(/"answer"\s*:\s*"((?:\\.|[^"\\])*)/s);
            if (match) {
              try {
                setDraft(JSON.parse('"' + match[1] + '"'));
              } catch {
                /* Wait for a complete escaped character. */
              }
            }
          }
          if (kind === 'complete') {
            const reply: Reply = JSON.parse(data);
            setReplies((previous) => [...previous, reply]);
            setDraft('');
            completed = true;
          }
        }
      }
      if (!completed)
        throw new Error(
          'Reviewer stream interrupted. The partial answer was not verified; retry.',
        );
      finishTask('succeeded');
    } catch (error) {
      finishTask(controller.signal.aborted ? 'cancelled' : 'failed');
      if (!controller.signal.aborted)
        setError(error instanceof Error ? error.message : 'Unable to answer');
    } finally {
      setBusy(false);
      setDraft('');
    }
  }
  const observations = integrity?.events ?? [];
  const signals = observations.filter(
    (event) =>
      integritySignalKinds.has(event.evidence.kind) &&
      !!event.attachments?.length,
  );
  const routine = observations.filter(
    (event) => !integritySignalKinds.has(event.evidence.kind),
  );
  const selectedSignal = signals.find((event) => event.id === selected);

  async function annotate() {
    setBusy(true);
    setError('');
    try {
      await request(
        `/api/v1/sessions/${session}/annotations`,
        {
          method: 'POST',
          body: JSON.stringify({ event_id: selected, decision, note }),
        },
        token,
      );
      setNote('');
      await refreshIntegrity();
    } catch (error) {
      setError(error instanceof Error ? error.message : 'Unable to annotate');
    } finally {
      setBusy(false);
    }
  }
  return (
    <section className="question-preparation">
      <h3 className="flex items-center justify-between gap-2">
        Live processing activity{' '}
        <HelpTip text="Live backend updates for this session, including candidate observations, evidence uploads and scoring progress." />
      </h3>
      <p role="status">{connection}</p>
      <ExpandableSection
        label={<>Recent events ({events.length}) </>}
        help="Shows only the latest 30 live backend events. Candidate observations also appear in the observations list; an evidence upload is a separate event."
      >
        <ol>
          {events.map((event) => (
            <li key={event.id}>
              {new Date(event.created_at).toLocaleTimeString()} ·{' '}
              {event.action.replaceAll('.', ' ')}
              {event.metadata.kind ? ` · ${String(event.metadata.kind)}` : ''}
              {event.action === 'assessment.question.scored' && (
                <ExpandableSection
                  label={<>Validated scoring progress (provisional)</>}
                >
                  <pre className="preserve">
                    {JSON.stringify(event.metadata.result, null, 2)}
                  </pre>
                </ExpandableSection>
              )}
            </li>
          ))}
        </ol>
      </ExpandableSection>
      <h3 className="flex items-center justify-between gap-2">
        Integrity review{' '}
        <HelpTip
          text={
            integrity?.notice ??
            'Recorded signals help prioritize human review.'
          }
        />
      </h3>
      {integrity && (
        <>
          <p>
            <strong>Review priority: {integrity.risk_score}/100</strong>
          </p>
          <p>{integrity.events.length} recorded observations</p>
          <ProctorEvidence
            label="Potential integrity deviations with media"
            showDecisions
            events={signals}
            session={session}
            token={token}
          />
          <ProctorEvidence
            label="Other observations & monitoring status"
            events={routine}
            session={session}
            token={token}
          />
          <div className="my-3">
            <RefreshButton
              pending={refreshing}
              onClick={() => {
                setRefreshing(true);
                void refreshIntegrity()
                  .catch(() => setError('Unable to refresh integrity'))
                  .finally(() => setRefreshing(false));
              }}
            >
              Refresh evidence
            </RefreshButton>
          </div>
          {signals.length === 0 && (
            <p>No potential integrity deviations with saved media.</p>
          )}
          {signals.length > 0 && (
            <div className="grid gap-3">
              <label>
                Potential integrity deviation
                <Select
                  value={selectedSignal?.id ?? ''}
                  onChange={(e) => setSelected(e.target.value)}
                >
                  <option value="">Choose an event</option>
                  {signals.map((event) => (
                    <option key={event.id} value={event.id}>
                      {new Date(event.received_at).toLocaleTimeString()} ·{' '}
                      {event.evidence.kind.replaceAll('_', ' ')}{' '}
                      {event.annotation ? `(${event.annotation.decision})` : ''}
                    </option>
                  ))}
                </Select>
              </label>
              <label>
                Review decision
                <Select
                  value={decision}
                  onChange={(e) => setDecision(e.target.value)}
                >
                  <option value="dismissed">Dismiss signal</option>
                  <option value="suspected">Needs further review</option>
                  <option value="confirmed_activity">
                    Activity confirmed (not a cheating verdict)
                  </option>
                </Select>
              </label>
              <label>
                Reason
                <textarea
                  rows={2}
                  maxLength={2000}
                  value={note}
                  onChange={(e) => setNote(e.target.value)}
                />
              </label>
            </div>
          )}
        </>
      )}
      <div className="mt-4 flex flex-wrap items-center gap-3">
        {signals.length > 0 && (
          <Button
            variant="secondary"
            disabled={busy || !selectedSignal || !note.trim()}
            onClick={() => void annotate()}
          >
            Save annotation
          </Button>
        )}
        <Button
          variant="secondary"
          disabled={busy}
          onClick={() => {
            const prompt =
              'Give a one-line summary of the integrity evidence. Distinguish observations from suspected cheating and cite the recorded evidence.';
            setQuestion(prompt);
            void ask(0, prompt);
          }}
        >
          Summarize integrity evidence
        </Button>
      </div>
      <h3>Ask about this interview</h3>
      <EvidenceIndex session={session} token={token} />
      <p className="muted">
        Answers use this session’s evidence only. Follow-up questions retain
        this conversation’s context.
      </p>
      {replies.map((reply, i) => (
        <article className="review-reply" key={i}>
          <p className="preserve">{reply.answer}</p>
          <ExpandableSection label={<>Retrieved sources</>}>
            {reply.sources.map((source) => (
              <ExpandableSection key={source.id} label={<>{source.id}</>}>
                <pre className="preserve">{source.text}</pre>
              </ExpandableSection>
            ))}
          </ExpandableSection>
        </article>
      ))}
      {draft && (
        <article className="review-reply" role="status">
          <small>Generating · citations not yet verified</small>
          <p className="preserve">{draft}</p>
        </article>
      )}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void ask();
        }}
      >
        <label>
          Question
          <textarea
            rows={2}
            maxLength={2000}
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            placeholder="Which answers were weakest against the rubric?"
          />
        </label>
        <div className="actions">
          <Button
            type="submit"
            variant="standard"
            disabled={busy || !question.trim()}
          >
            {busy ? 'Retrieving evidence…' : 'Ask assistant'}
          </Button>
          {replies.at(-1)?.next_page != null && (
            <Button
              type="button"
              variant="secondary"
              disabled={busy}
              onClick={() => void ask(replies.at(-1)!.next_page!)}
            >
              Next evidence page
            </Button>
          )}
        </div>
      </form>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
