// Display helpers — turn wire-shape enums and discriminator
// strings into UI-ready labels. Slice 9's validation viewer
// composes these into accessible badges; slice 8's object
// browser uses the statement-kind labels in tables.

import type { ValidationIssueSeverity, ValidationStatus } from '@kairo/api-client';

/** Statement-kind discriminators the daemon emits on the
 * `type` field of any `*StatementJson`. Listed for the v1 set
 * the daemon currently surfaces. */
export const KNOWN_STATEMENT_KINDS = [
  'ActorGenesis',
  'ObjectGenesis',
  'ObjectRevision',
  'ObjectBranch',
  'ObjectVersionTag',
  'ActorTrust',
  'ActorCapabilityGrant',
  'ActorCapabilityRevocation',
  'ActorKeyRotation',
  'ActorKeyRevocation',
  'ActorEmergencyKeyRotation',
  'ActorEmergencyKeyRevocation',
  'ActorAttestationKeyAdd',
  'ActorAttestationKeyRevocation',
  'ActorAttestationThresholdChange',
] as const;

export type StatementKind = (typeof KNOWN_STATEMENT_KINDS)[number];

/** Locality of a piece of data. Kept distinct from validity per
 * `WEB_CLIENT.md` §15 — an object can be local and invalid, or
 * remote and valid. v1 only emits `local`; the others are
 * reserved for the federation surface. */
export type LocalityState =
  | 'local'
  | 'remote'
  | 'cached'
  | 'pinned'
  | 'partial'
  | 'missing'
  | 'fetching'
  | 'unavailable';

/** Human-readable label for a `ValidationStatus`. Title case;
 * never color-only. */
export function validationStatusLabel(status: ValidationStatus): string {
  switch (status) {
    case 'valid':
      return 'Valid';
    case 'invalid':
      return 'Invalid';
    case 'conflicted':
      return 'Conflicted';
    case 'indeterminate':
      return 'Indeterminate';
    case 'unverified':
      return 'Unverified';
  }
}

/** One-sentence description of what each validation status
 * means; safe to put in a tooltip or hover hint. */
export function validationStatusDescription(status: ValidationStatus): string {
  switch (status) {
    case 'valid':
      return 'All checks passed.';
    case 'invalid':
      return 'At least one check failed.';
    case 'conflicted':
      return 'Multiple actors disagree on this object.';
    case 'indeterminate':
      return 'Not enough data to determine validity.';
    case 'unverified':
      return 'Verification was not run.';
  }
}

/** Label for a validation issue's severity. */
export function severityLabel(severity: ValidationIssueSeverity): string {
  switch (severity) {
    case 'info':
      return 'Info';
    case 'warning':
      return 'Warning';
    case 'error':
      return 'Error';
  }
}

/** Title-case label for a statement kind. Falls back to the raw
 * value for kinds the frontend doesn't recognize, so a daemon-
 * side schema addition shows up cleanly until the label table
 * is updated. */
export function statementKindLabel(kind: string): string {
  switch (kind) {
    case 'ActorGenesis':
      return 'Actor genesis';
    case 'ObjectGenesis':
      return 'Object genesis';
    case 'ObjectRevision':
      return 'Object revision';
    case 'ObjectBranch':
      return 'Branch tip';
    case 'ObjectVersionTag':
      return 'Version tag';
    case 'ActorTrust':
      return 'Trust opinion';
    case 'ActorCapabilityGrant':
      return 'Capability grant';
    case 'ActorCapabilityRevocation':
      return 'Capability revocation';
    case 'ActorKeyRotation':
      return 'Key rotation';
    case 'ActorKeyRevocation':
      return 'Key revocation';
    case 'ActorEmergencyKeyRotation':
      return 'Emergency key rotation';
    case 'ActorEmergencyKeyRevocation':
      return 'Emergency key revocation';
    case 'ActorAttestationKeyAdd':
      return 'Attestation key add';
    case 'ActorAttestationKeyRevocation':
      return 'Attestation key revocation';
    case 'ActorAttestationThresholdChange':
      return 'Attestation threshold change';
    default:
      return kind;
  }
}

/** Label for a `LocalityState`. */
export function localityLabel(state: LocalityState): string {
  switch (state) {
    case 'local':
      return 'Local';
    case 'remote':
      return 'Remote';
    case 'cached':
      return 'Cached';
    case 'pinned':
      return 'Pinned';
    case 'partial':
      return 'Partial';
    case 'missing':
      return 'Missing';
    case 'fetching':
      return 'Fetching';
    case 'unavailable':
      return 'Unavailable';
  }
}
