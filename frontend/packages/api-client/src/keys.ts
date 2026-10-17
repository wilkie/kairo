// Centralized TanStack Query keys (`WEB_CLIENT.md` §7.1).
//
// Every cache entry the inspector reads goes through this
// factory. Co-locating the keys keeps invalidation /
// prefetching trivial: a slice that needs to refresh "every
// branch view of an object" can call
// `queryClient.invalidateQueries({ queryKey: kairoKeys.branches(objectId) })`.

export const kairoKeys = {
  all: ['kairo'] as const,

  daemon: () => [...kairoKeys.all, 'daemon'] as const,
  daemonVersion: () => [...kairoKeys.daemon(), 'version'] as const,
  daemonStatus: () => [...kairoKeys.daemon(), 'status'] as const,

  actors: () => [...kairoKeys.all, 'actors'] as const,
  actor: (id: string) => [...kairoKeys.actors(), id] as const,
  actorStatements: (id: string) => [...kairoKeys.actor(id), 'statements'] as const,
  actorObjects: (id: string) => [...kairoKeys.actor(id), 'objects'] as const,

  objects: () => [...kairoKeys.all, 'objects'] as const,
  object: (id: string) => [...kairoKeys.objects(), id] as const,

  statements: () => [...kairoKeys.all, 'statements'] as const,
  statement: (id: string) => [...kairoKeys.statements(), id] as const,

  branches: (objectId: string) => [...kairoKeys.object(objectId), 'branches'] as const,
  branch: (objectId: string, name: string, actor?: string) =>
    [...kairoKeys.branches(objectId), name, actor ?? null] as const,

  versionTags: (objectId: string) => [...kairoKeys.object(objectId), 'version-tags'] as const,
  versionTag: (objectId: string, version: string, actor?: string) =>
    [...kairoKeys.object(objectId), 'version-tag', version, actor ?? null] as const,

  revisionLog: (objectId: string, from: ReadonlyArray<string>, pageSize: number) =>
    [...kairoKeys.object(objectId), 'revision-log', [...from], pageSize] as const,

  trust: (byActor: string, ofActor: string) =>
    [...kairoKeys.all, 'trust', byActor, ofActor] as const,
  trustAbout: (ofActor: string) => [...kairoKeys.all, 'trust', 'about', ofActor] as const,

  capabilitiesFrom: (grantor: string) =>
    [...kairoKeys.all, 'capabilities', 'from', grantor] as const,
  capabilitiesForObject: (objectId: string) =>
    [...kairoKeys.object(objectId), 'capabilities'] as const,

  verifyObject: (id: string) => [...kairoKeys.object(id), 'verify'] as const,

  blob: (id: string) => [...kairoKeys.all, 'blobs', id] as const,

  /** Keyed by typed reference (`statement:<id>`, `object:<id>`, …). */
  provenance: (record: string) => [...kairoKeys.all, 'provenance', record] as const,

  session: () => [...kairoKeys.all, 'session'] as const,
  sessionCurrent: () => [...kairoKeys.session(), 'current'] as const,
  keystoreActors: () => [...kairoKeys.session(), 'keystore-actors'] as const,
} as const;
