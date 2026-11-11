import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useMemo, useState } from 'react';
import { LoaderCircle, CheckCircle2 } from 'lucide-react';
import { Button } from '../../components/ui/button';
import { workspaceApi } from './api';

/** The server only releases a link after the entire private set is validated. */
export function CandidatePreparation({
  token,
  session,
}: {
  token: string;
  session: string;
}) {
  const api = useMemo(() => workspaceApi(token), [token]);
  const client = useQueryClient();
  const [copied, setCopied] = useState(false);
  const key = ['candidate-preparation', token, session];
  const preparation = useQuery({
    queryKey: key,
    queryFn: ({ signal }) => api.generation(session, signal),
    refetchInterval: (query) => {
      const status = query.state.data?.status;
      return status === 'pending' || status === 'running' ? 2000 : false;
    },
  });
  const ready = preparation.data?.status === 'completed';
  const invitation = useQuery({
    queryKey: ['prepared-invitation', token, session],
    queryFn: () => api.invitation(session),
    enabled: ready,
    staleTime: Infinity,
  });
  const retry = useMutation({
    mutationFn: () => api.retryGeneration(session),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: key });
      await client.invalidateQueries({ queryKey: ['workspace-view', token] });
    },
  });
  useEffect(() => {
    if (ready || preparation.data?.status === 'failed') {
      void client.invalidateQueries({ queryKey: ['workspace-view', token] });
    }
  }, [client, token, ready, preparation.data?.status]);
  const link = invitation.data
    ? `${window.location.origin}${window.location.pathname}#invite=${invitation.data.invitation_token}`
    : '';
  const error = preparation.error ?? invitation.error ?? retry.error;
  const failed = preparation.data?.status === 'failed';

  return (
    <section
      className="grid gap-4 rounded-xl border border-indigo-500/20 bg-indigo-500/5 p-4"
      aria-label="Candidate question preparation"
    >
      <div className="flex items-center gap-2" role="status">
        {ready ? (
          <CheckCircle2 className="size-4 text-emerald-500" />
        ) : (
          !failed && (
            <LoaderCircle className="size-4 motion-safe:animate-spin" />
          )
        )}
        <h3>
          {ready
            ? 'Invitation ready'
            : failed
              ? 'Preparation failed'
              : 'Preparing questions'}
        </h3>
      </div>
      {!ready && (
        <p className="text-detail text-slate-600 workspace-dark:text-slate-300">
          {preparation.data?.questions.length ?? 0}
          {preparation.data?.job?.planned_total
            ? ` of ${preparation.data.job.planned_total}`
            : ''}{' '}
          questions saved. A private link will appear after all questions pass
          validation. You can close this window and return from the candidate’s
          session details.
        </p>
      )}
      {preparation.data?.job?.last_error && (
        <p className="text-detail">{preparation.data.job.last_error}</p>
      )}
      {!ready && preparation.data?.configured === false && (
        <p role="status">
          AI configuration is required. Preparation will wait until the service
          is available.
        </p>
      )}
      {failed && (
        <Button disabled={retry.isPending} onClick={() => retry.mutate()}>
          {retry.isPending ? 'Queuing…' : 'Retry preparation'}
        </Button>
      )}
      {error && (
        <p role="alert" className="error">
          {error.message}
        </p>
      )}
      {(preparation.isError || invitation.isError) && (
        <Button
          onClick={() =>
            void (ready ? invitation.refetch() : preparation.refetch())
          }
        >
          Retry loading
        </Button>
      )}
      {link && (
        <>
          <label className="grid gap-2 text-detail">
            Candidate invitation link
            <textarea
              className="w-full"
              readOnly
              rows={3}
              value={link}
              onFocus={(event) => event.currentTarget.select()}
            />
          </label>
          <p className="text-detail text-slate-600 workspace-dark:text-slate-300">
            Expires seven days after preparation completes. Share this link
            manually.
          </p>
          <Button
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(link);
                setCopied(true);
              } catch {
                setCopied(false);
              }
            }}
          >
            {copied ? 'Copied' : 'Copy invitation link'}
          </Button>
        </>
      )}
    </section>
  );
}
