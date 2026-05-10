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

import { useQuery, type UseQueryResult } from '@tanstack/react-query';
import type { KairoApiClientError } from './error';
import type {
  ActorGenesisJson,
  ActorTrustStatementJson,
  BranchActorOption,
  BranchTipDto,
  CapabilityHeadDto,
  ObjectBranchStatementJson,
  ObjectGenesisStatementJson,
  ObjectVersionTagStatementJson,
  RevisionHeadDto,
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
