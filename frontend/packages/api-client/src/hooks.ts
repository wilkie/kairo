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

export function useObjectsByActor(
  actorId: string,
  opts: { enabled?: boolean } = {},
): QueryResult<ObjectByActorDto[]> {
  const client = useKairoClient();
  return useQuery<ObjectByActorDto[], KairoApiClientError>({
    queryKey: kairoKeys.actorObjects(actorId),
    queryFn: () => client.listObjectsByActor(actorId),
    enabled: opts.enabled ?? true,
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
 * Variables accepted by [`useUpdateBranch`]. The mutation
 * sends an `ObjectBranch` body — the daemon's capability gate
 * decides whether the session actor may write a branch on the
 * named object. `supersedes` is optional: pass it when
 * replacing an existing tip from the same actor with the same
 * name; omit it to create a fresh branch.
 */
export interface UpdateBranchVars {
  byActor: string;
  objectId: string;
  name: string;
  /** Statement id of the revision the branch tip should
   * point at. Resolves to `ObjectBranchBody.revision` on the
   * daemon side. */
  revisionStatementId: string;
  /** Statement id of the prior branch tip this update
   * supersedes. Omit when creating a new branch. */
  supersedes?: string;
}

/**
 * Mutation: cookie-mode write of an `ObjectBranch` statement.
 * On success invalidates the branches list and per-tip
 * resolution queries for the object so panels re-render with
 * the new head, plus the by-actor signed-statements list so
 * the actor-detail page reflects the new authorship.
 *
 * The daemon's capability gate is the source of truth for
 * whether the session actor may write a branch — denials
 * surface as `ApiClientError` with `code: 'not_authorized'`
 * and the consuming dialog renders them inline.
 */
export function useUpdateBranch(): UseMutationResult<
  PutStatementResult,
  KairoApiClientError,
  UpdateBranchVars
> {
  const client = useKairoClient();
  const queryClient = useQueryClient();
  return useMutation<PutStatementResult, KairoApiClientError, UpdateBranchVars>({
    mutationFn: ({ objectId, name, revisionStatementId, supersedes }) => {
      const body: Record<string, unknown> = {
        object: objectId,
        name,
        revision: revisionStatementId,
      };
      if (supersedes !== undefined) {
        body.supersedes = supersedes;
      }
      return client.writeViaSession('ObjectBranch', body);
    },
    onSuccess: (_data, vars) => {
      void queryClient.invalidateQueries({
        queryKey: kairoKeys.branches(vars.objectId),
      });
      void queryClient.invalidateQueries({
        queryKey: kairoKeys.branch(vars.objectId, vars.name, vars.byActor),
      });
      void queryClient.invalidateQueries({
        queryKey: kairoKeys.actorStatements(vars.byActor),
      });
    },
  });
}

/**
 * Variables accepted by [`useSetVersionTag`]. The mutation
 * sends a binding `ObjectVersionTag` body (`target` set) — the
 * daemon's capability gate decides whether the session actor
 * may tag the named object. `supersedes` is optional: pass the
 * actor's current head for this version when retargeting; omit
 * it for the version's first tag.
 */
export interface SetVersionTagVars {
  byActor: string;
  objectId: string;
  /** Strict semver 2.0.0 string (no leading `v`). */
  version: string;
  /** Statement id of the revision the tag should point at.
   * Resolves to `ObjectVersionTagBody.target` on the daemon
   * side. */
  revisionStatementId: string;
  /** Statement id of the prior tag for `(byActor, version)`
   * this one supersedes. Omit for a first tag. */
  supersedes?: string;
}

/**
 * Mutation: cookie-mode write of a binding `ObjectVersionTag`
 * statement. On success invalidates the tag list and the
 * per-version resolution for the object, plus the by-actor
 * signed-statements list.
 *
 * Revocations (`target` omitted) are not exposed here yet; see
 * `PHASE_2_WEB_WRITES.md` slice 5.
 */
export function useSetVersionTag(): UseMutationResult<
  PutStatementResult,
  KairoApiClientError,
  SetVersionTagVars
> {
  const client = useKairoClient();
  const queryClient = useQueryClient();
  return useMutation<PutStatementResult, KairoApiClientError, SetVersionTagVars>({
    mutationFn: ({ objectId, version, revisionStatementId, supersedes }) => {
      const body: Record<string, unknown> = {
        object: objectId,
        version,
        target: revisionStatementId,
      };
      if (supersedes !== undefined) {
        body.supersedes = supersedes;
      }
      return client.writeViaSession('ObjectVersionTag', body);
    },
    onSuccess: (_data, vars) => {
      void queryClient.invalidateQueries({
        queryKey: kairoKeys.versionTags(vars.objectId),
      });
      // The per-version key defaults its actor to the object's
      // creator when unset, so invalidate both spellings.
      void queryClient.invalidateQueries({
        queryKey: kairoKeys.versionTag(vars.objectId, vars.version),
      });
      void queryClient.invalidateQueries({
        queryKey: kairoKeys.versionTag(vars.objectId, vars.version, vars.byActor),
      });
      void queryClient.invalidateQueries({
        queryKey: kairoKeys.actorStatements(vars.byActor),
      });
    },
  });
}

/**
 * Variables accepted by [`useCreateObject`]. Only the kind is
 * caller-supplied: the daemon fills `created_by` (the session
 * actor), `created_at`, and a fresh nonce, which derive the new
 * `ObjectId`.
 */
export interface CreateObjectVars {
  byActor: string;
  objectKind: string;
}

/**
 * Mutation: cookie-mode write of an `ObjectGenesis`. The result's
 * `statement_id` is the new object's id. Genesis needs no
 * capability — the signer becomes the object's root authority.
 * On success invalidates the creator's Created objects list.
 */
export function useCreateObject(): UseMutationResult<
  PutStatementResult,
  KairoApiClientError,
  CreateObjectVars
> {
  const client = useKairoClient();
  const queryClient = useQueryClient();
  return useMutation<PutStatementResult, KairoApiClientError, CreateObjectVars>({
    mutationFn: ({ objectKind }) =>
      client.writeViaSession('ObjectGenesis', { object_kind: objectKind }),
    onSuccess: (_data, vars) => {
      void queryClient.invalidateQueries({ queryKey: kairoKeys.actorObjects(vars.byActor) });
    },
  });
}

/** Statement kinds an object-scoped capability can carry
 * (`CAPABILITIES.md` §4.3). */
export type ObjectCapabilityKind = 'ObjectRevision' | 'ObjectBranch' | 'ObjectVersionTag';

/**
 * Variables accepted by [`useGrantCapability`]. Sends an
 * object-scoped `ActorCapabilityGrant`. Each `(grantor, grantee,
 * scope)` triple has one chain, and its leaf's kind list is the
 * complete authorized set (`CAPABILITIES.md` §5.1.1): to change an
 * existing grant, pass the *full* new list and the current head as
 * `supersedes`.
 */
export interface GrantCapabilityVars {
  byActor: string;
  objectId: string;
  grantee: string;
  statementKinds: ObjectCapabilityKind[];
  delegable: boolean;
  /** RFC 3339 UTC seconds; becomes an `expires_at` constraint. */
  expiresAt?: string;
  /** Statement id of the current head for this triple. Omit for
   * the triple's first grant. */
  supersedes?: string;
}

/**
 * Mutation: cookie-mode write of an `ActorCapabilityGrant`. The
 * daemon checks the session actor is the object's root authority
 * or holds every granted kind with `delegable = true`. On success
 * invalidates the object's capability heads (which also drive the
 * Authority chains panel), the grantor's list, and the grantor's
 * signed-statements list.
 */
export function useGrantCapability(): UseMutationResult<
  PutStatementResult,
  KairoApiClientError,
  GrantCapabilityVars
> {
  const client = useKairoClient();
  const queryClient = useQueryClient();
  return useMutation<PutStatementResult, KairoApiClientError, GrantCapabilityVars>({
    mutationFn: ({ objectId, grantee, statementKinds, delegable, expiresAt, supersedes }) => {
      const body: Record<string, unknown> = {
        grantee,
        capability: {
          scope: { object: objectId },
          statement_kinds: statementKinds,
          delegable,
          constraints: expiresAt !== undefined ? [{ expires_at: expiresAt }] : [],
        },
      };
      if (supersedes !== undefined) {
        body.supersedes = supersedes;
      }
      return client.writeViaSession('ActorCapabilityGrant', body);
    },
    onSuccess: (_data, vars) => invalidateCapabilityQueries(queryClient, vars),
  });
}

/**
 * Variables accepted by [`useRevokeCapability`]. `retroactive`
 * invalidates the grant from inception, along with everything
 * signed under it (`CAPABILITIES.md` §6.3) — reserve it for grant
 * fraud or grantee key compromise.
 */
export interface RevokeCapabilityVars {
  byActor: string;
  /** Object the grant is scoped to; used for cache invalidation. */
  objectId: string;
  revokedGrant: string;
  retroactive: boolean;
  reason?: string;
}

/**
 * Mutation: cookie-mode write of an `ActorCapabilityRevocation`.
 * Only the original grantor may revoke; the daemon enforces it.
 */
export function useRevokeCapability(): UseMutationResult<
  PutStatementResult,
  KairoApiClientError,
  RevokeCapabilityVars
> {
  const client = useKairoClient();
  const queryClient = useQueryClient();
  return useMutation<PutStatementResult, KairoApiClientError, RevokeCapabilityVars>({
    mutationFn: ({ revokedGrant, retroactive, reason }) => {
      const body: Record<string, unknown> = { revoked_grant: revokedGrant, retroactive };
      if (reason !== undefined && reason.length > 0) {
        body.reason = reason;
      }
      return client.writeViaSession('ActorCapabilityRevocation', body);
    },
    onSuccess: (_data, vars) => invalidateCapabilityQueries(queryClient, vars),
  });
}

function invalidateCapabilityQueries(
  queryClient: ReturnType<typeof useQueryClient>,
  vars: { byActor: string; objectId: string },
): void {
  void queryClient.invalidateQueries({ queryKey: kairoKeys.capabilitiesForObject(vars.objectId) });
  void queryClient.invalidateQueries({ queryKey: kairoKeys.capabilitiesFrom(vars.byActor) });
  void queryClient.invalidateQueries({ queryKey: kairoKeys.actorStatements(vars.byActor) });
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
