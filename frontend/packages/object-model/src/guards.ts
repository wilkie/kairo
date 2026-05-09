// Type guards for polymorphic API payloads. The daemon's
// `/api/v1/statements/{id}` endpoint returns *any* signed
// statement variant; the kind discriminator lives on the
// `type` field of the body itself. These guards narrow that
// payload back to a typed shape so call sites can switch on
// the kind without a third-party schema validator at the
// boundary.

import type {
  ActorTrustStatementJson,
  ObjectBranchStatementJson,
  ObjectGenesisStatementJson,
  ObjectVersionTagStatementJson,
  StatementValue,
} from '@kairo/api-client';

/** Narrow any `StatementValue` to a kind, or return `null`
 * when the payload doesn't have a recognizable discriminator.
 * Call sites use it like: `const t = statementType(value);
 * if (t === 'ObjectBranch') { ... }`. */
export function statementType(value: StatementValue): string | null {
  if (value === null || typeof value !== 'object') {
    return null;
  }
  const candidate = (value as { type?: unknown }).type;
  return typeof candidate === 'string' ? candidate : null;
}

export function isObjectGenesisStatement(
  value: StatementValue,
): value is ObjectGenesisStatementJson {
  return statementType(value) === 'ObjectGenesis';
}

export function isObjectBranchStatement(
  value: StatementValue,
): value is ObjectBranchStatementJson {
  return statementType(value) === 'ObjectBranch';
}

export function isObjectVersionTagStatement(
  value: StatementValue,
): value is ObjectVersionTagStatementJson {
  return statementType(value) === 'ObjectVersionTag';
}

export function isActorTrustStatement(
  value: StatementValue,
): value is ActorTrustStatementJson {
  return statementType(value) === 'ActorTrust';
}
