// TanStack Query hooks, one per daemon endpoint.
//
// Pattern for every hook: pull the api-client out of
// `useKairoClient`, wire a stable key from `kairoKeys`, let
// consumers branch on the `data | error | isPending` triple
// TanStack Query returns. Errors land as `KairoApiClientError`
// so React code can switch on the discriminated `detail.kind`
// without generic-error guesswork.
//
// Slice 6 ships hooks for all 12 daemon endpoints except the
// blob endpoint, which streams binary data — that one stays as
// the imperative `client.getBlob(id)` because raw `Blob` results
// don't fit TanStack Query's caching model cleanly. Image / text
// preview components in slice 10 will manage blob fetches with
// their own object-URL lifecycle.

import {
  useMutation,
  useQuery,
  useQueryClient,
  type UseMutationResult,
  type UseQueryResult,
} from '@tanstack/react-query';
import type { KairoApiClientError } from './error';
import type {
  ActorGenesisJson,
  ActorTrustStatementJson,
  BranchActorOption,
  BranchTipDto,
  CapabilityHeadDto,
  KeystoreActorDto,
  ObjectBranchStatementJson,
  ObjectByActorDto,
  ObjectGenesisStatementJson,
  ObjectVersionTagStatementJson,
  PutStatementResult,
  RevisionHeadDto,
  SessionInfo,
  StatementByActorDto,
  StatementValue,
  StatusInfo,
  TrustHeadDto,
  ValidationResult,
  VersionInfo,
  VersionTagHeadDto,
} from './client';
import { kairoKeys } from './keys';
import { useKairoClient } from './provider';

type QueryResult<T> = UseQueryResult<T, KairoApiClientError>;

export function useVersion(): QueryResult<VersionInfo> {
  const client = useKairoClient();
  return useQuery<VersionInfo, KairoApiClientError>({
    queryKey: kairoKeys.daemonVersion(),
    queryFn: () => client.getVersion(),
  });
}

export function useDaemonStatus(): QueryResult<StatusInfo> {
  const client = useKairoClient();
  return useQuery<StatusInfo, KairoApiClientError>({
    queryKey: kairoKeys.daemonStatus(),
    queryFn: () => client.getStatus(),
  });
}

export function useActor(actorId: string): QueryResult<ActorGenesisJson> {
  const client = useKairoClient();
  return useQuery<ActorGenesisJson, KairoApiClientError>({
    queryKey: kairoKeys.actor(actorId),
    queryFn: () => client.getActor(actorId),
  });
}

export function useStatementsByActor(actorId: string): QueryResult<StatementByActorDto[]> {
  const client = useKairoClient();
  return useQuery<StatementByActorDto[], KairoApiClientError>({
    queryKey: kairoKeys.actorStatements(actorId),
    queryFn: () => client.listStatementsByActor(actorId),
  });
}

export function useObjectsByActor(actorId: string): QueryResult<ObjectByActorDto[]> {
  const client = useKairoClient();
  return useQuery<ObjectByActorDto[], KairoApiClientError>({
    queryKey: kairoKeys.actorObjects(actorId),
    queryFn: () => client.listObjectsByActor(actorId),
  });
}

export function useObject(objectId: string): QueryResult<ObjectGenesisStatementJson> {
  const client = useKairoClient();
  return useQuery<ObjectGenesisStatementJson, KairoApiClientError>({
    queryKey: kairoKeys.object(objectId),
    queryFn: () => client.getObject(objectId),
  });
}

export function useStatement(statementId: string): QueryResult<StatementValue> {
  const client = useKairoClient();
  return useQuery<StatementValue, KairoApiClientError>({
    queryKey: kairoKeys.statement(statementId),
    queryFn: () => client.getStatement(statementId),
  });
}

export function useBranches(objectId: string): QueryResult<BranchTipDto[]> {
  const client = useKairoClient();
  return useQuery<BranchTipDto[], KairoApiClientError>({
    queryKey: kairoKeys.branches(objectId),
    queryFn: () => client.listBranches(objectId),
  });
}

export function useLatestBranch(
  objectId: string,
  name: string,
  opts: BranchActorOption = {},
): QueryResult<ObjectBranchStatementJson> {
  const client = useKairoClient();
  return useQuery<ObjectBranchStatementJson, KairoApiClientError>({
    queryKey: kairoKeys.branch(objectId, name, opts.actor),
    queryFn: () => client.getLatestBranch(objectId, name, opts),
  });
}

export function useVersionTags(objectId: string): QueryResult<VersionTagHeadDto[]> {
  const client = useKairoClient();
  return useQuery<VersionTagHeadDto[], KairoApiClientError>({
    queryKey: kairoKeys.versionTags(objectId),
    queryFn: () => client.listVersionTags(objectId),
  });
}

export function useLatestVersionTag(
  objectId: string,
  version: string,
  opts: BranchActorOption = {},
): QueryResult<ObjectVersionTagStatementJson> {
  const client = useKairoClient();
  return useQuery<ObjectVersionTagStatementJson, KairoApiClientError>({
    queryKey: kairoKeys.versionTag(objectId, version, opts.actor),
    queryFn: () => client.getLatestVersionTag(objectId, version, opts),
  });
}

export function useRevisions(objectId: string): QueryResult<RevisionHeadDto[]> {
  const client = useKairoClient();
  return useQuery<RevisionHeadDto[], KairoApiClientError>({
    queryKey: kairoKeys.revisions(objectId),
    queryFn: () => client.listRevisions(objectId),
  });
}

export function useTrust(byActor: string, ofActor: string): QueryResult<ActorTrustStatementJson> {
  const client = useKairoClient();
  return useQuery<ActorTrustStatementJson, KairoApiClientError>({
    queryKey: kairoKeys.trust(byActor, ofActor),
    queryFn: () => client.getTrust(byActor, ofActor),
  });
}

export interface UseTrustAboutOptions {
  /** Skip the request when false. Use for chained queries where
   * the actor id only becomes available after a parent query
   * resolves (e.g., reading `object.body.created_by` before
   * fetching trust opinions about that actor). */
  enabled?: boolean;
}

export function useTrustAbout(
  ofActor: string,
  opts: UseTrustAboutOptions = {},
): QueryResult<TrustHeadDto[]> {
  const client = useKairoClient();
  return useQuery<TrustHeadDto[], KairoApiClientError>({
    queryKey: kairoKeys.trustAbout(ofActor),
    queryFn: () => client.listTrustAbout(ofActor),
    enabled: opts.enabled !== false,
  });
}

export function useCapabilitiesFromGrantor(grantorId: string): QueryResult<CapabilityHeadDto[]> {
  const client = useKairoClient();
  return useQuery<CapabilityHeadDto[], KairoApiClientError>({
    queryKey: kairoKeys.capabilitiesFrom(grantorId),
    queryFn: () => client.listCapabilitiesFromGrantor(grantorId),
  });
}

export function useCapabilitiesForObject(objectId: string): QueryResult<CapabilityHeadDto[]> {
  const client = useKairoClient();
  return useQuery<CapabilityHeadDto[], KairoApiClientError>({
    queryKey: kairoKeys.capabilitiesForObject(objectId),
    queryFn: () => client.listCapabilitiesForObject(objectId),
  });
}

export function useVerifyObject(objectId: string): QueryResult<ValidationResult> {
  const client = useKairoClient();
  return useQuery<ValidationResult, KairoApiClientError>({
    queryKey: kairoKeys.verifyObject(objectId),
    queryFn: () => client.verifyObject(objectId),
  });
}

/**
 * `GET /api/v1/keystore/actors` — actors the local daemon can
 * sign for. Used by `<ActorPicker />`. Returns the same array
 * the daemon serves, sorted by actor id.
 */
export function useKeystoreActors(): QueryResult<KeystoreActorDto[]> {
  const client = useKairoClient();
  return useQuery<KeystoreActorDto[], KairoApiClientError>({
    queryKey: kairoKeys.keystoreActors(),
    queryFn: () => client.listKeystoreActors(),
  });
}

/**
 * `GET /api/v1/sessions/current` — active session metadata, or
 * `null` for the anonymous viewer. 401 from the daemon is
 * normalised to `null` by the client, so consumers can treat
 * "signed in or not" as a single branch on `data`.
 *
 * Refetches on window focus so an idle-expired session
 * surfaces within one focus cycle (no tight polling).
 */
export function useSession(): QueryResult<SessionInfo | null> {
  const client = useKairoClient();
  return useQuery<SessionInfo | null, KairoApiClientError>({
    queryKey: kairoKeys.sessionCurrent(),
    queryFn: () => client.getCurrentSession(),
    refetchOnWindowFocus: true,
    staleTime: 5 * 60 * 1000, // 5 min; window-focus invalidates anyway
  });
}

/**
 * Mutation: `POST /api/v1/sessions { actor_id }`. On success,
 * invalidates the session-current query so observers re-render
 * with the new session in one cycle.
 */
export function useSignIn(): UseMutationResult<
  SessionInfo,
  KairoApiClientError,
  { actorId: string }
> {
  const client = useKairoClient();
  const queryClient = useQueryClient();
  return useMutation<SessionInfo, KairoApiClientError, { actorId: string }>({
    mutationFn: ({ actorId }) => client.openSession(actorId),
    onSuccess: (info) => {
      queryClient.setQueryData<SessionInfo>(kairoKeys.sessionCurrent(), info);
    },
  });
}

/**
 * Mutation: `DELETE /api/v1/sessions/current`. On success,
 * clears the session-current cache to `null`.
 */
export function useSignOut(): UseMutationResult<void, KairoApiClientError, void> {
  const client = useKairoClient();
  const queryClient = useQueryClient();
  return useMutation<void, KairoApiClientError, void>({
    mutationFn: () => client.closeSession(),
    onSuccess: () => {
      queryClient.setQueryData<SessionInfo | null>(kairoKeys.sessionCurrent(), null);
    },
  });
}

/**
 * Variables accepted by [`useCreateTrustOpinion`]. `byActor`
 * is the signed-in actor (passed in by callers so the hook
 * doesn't need to read the session itself, keeping it
 * dependency-free of session state).
 */
export interface CreateTrustOpinionVars {
  byActor: string;
  trustedActor: string;
  decision: 'trusted' | 'untrusted';
  reason?: string;
}

/**
 * Mutation: cookie-mode write of an `ActorTrust` statement
 * (the simplest body kind — no capability check). Wraps
 * `client.writeViaSession("ActorTrust", body)`; on success,
 * invalidates the trust queries that surface the new opinion
 * so consumers re-fetch and re-render.
 */
export function useCreateTrustOpinion(): UseMutationResult<
  PutStatementResult,
  KairoApiClientError,
  CreateTrustOpinionVars
> {
  const client = useKairoClient();
  const queryClient = useQueryClient();
  return useMutation<PutStatementResult, KairoApiClientError, CreateTrustOpinionVars>({
    mutationFn: ({ trustedActor, decision, reason }) => {
      const body: Record<string, unknown> = {
        trusted_actor: trustedActor,
        decision,
      };
      if (reason !== undefined && reason.length > 0) {
        body.reason = reason;
      }
      return client.writeViaSession('ActorTrust', body);
    },
    onSuccess: (_data, vars) => {
      void queryClient.invalidateQueries({
        queryKey: kairoKeys.trustAbout(vars.trustedActor),
      });
      void queryClient.invalidateQueries({
        queryKey: kairoKeys.trust(vars.byActor, vars.trustedActor),
      });
      // The new trust statement is also surfaced via the
      // by-actor signed-statements list — invalidate so the
      // actor-detail page's signed-statements panel reflects
      // the addition on next render.
      void queryClient.invalidateQueries({
        queryKey: kairoKeys.actorStatements(vars.byActor),
      });
    },
  });
}
