import type { components } from '@kairo/api-client';

type ValidationStatus = components['schemas']['ValidationStatus'];

export type ValidationBadgeProps = {
  status: ValidationStatus;
};

/**
 * Slice 5 placeholder. Slice 9 ships the real badge — accessible,
 * never-color-only per `WEB_CLIENT.md` §10/§20, with explicit
 * text labels for every status.
 */
export function ValidationBadge({ status }: ValidationBadgeProps) {
  return <span data-validation-status={status}>{status}</span>;
}
