// `@kairo/object-model` — display helpers + DTO re-exports for
// the inspector. No semantic validation lives here; the daemon
// (and `kairo-core`) own that.
//
// Slice 6 surface:
//
// - DTO re-exports under stable, ergonomic names.
// - Identifier formatters (`truncateId`, `copyToClipboard`).
// - Display helpers for `ValidationStatus`, severity,
//   statement kinds, locality.
// - Type guards for the polymorphic statement payload.

// DTO re-exports — callers depend on @kairo/object-model
// instead of digging into @kairo/api-client's generated
// types directly. This is the boundary where presentation
// helpers and API types meet.
export type {
  VersionInfo,
  StatusInfo,
  ActorGenesisJson,
  ObjectGenesisStatementJson,
  ObjectBranchStatementJson,
  ObjectVersionTagStatementJson,
  ActorTrustStatementJson,
  BranchTipDto,
  CapabilityHeadDto,
  VersionTagHeadDto,
  RevisionHeadDto,
  TrustHeadDto,
  StatementValue,
  ValidationResult,
  ValidationStatus,
  ValidationIssue,
  ValidationIssueSeverity,
} from '@kairo/api-client';

export {
  truncateId,
  copyToClipboard,
  type TruncateIdOptions,
  type IdKind,
  idPrefix,
  isCanonicalId,
  canonicalizeId,
  bareId,
} from './identifiers';

export {
  KNOWN_STATEMENT_KINDS,
  validationStatusLabel,
  validationStatusDescription,
  severityLabel,
  statementKindLabel,
  localityLabel,
  type StatementKind,
  type LocalityState,
} from './display';

export {
  statementType,
  isObjectGenesisStatement,
  isObjectBranchStatement,
  isObjectVersionTagStatement,
  isActorTrustStatement,
} from './guards';
