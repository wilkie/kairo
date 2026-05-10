// Validation status badge — per `WEB_CLIENT.md` §10/§20 the
// status text always carries the meaning so the badge is
// legible without color (color-only is forbidden). Wraps
// `StatusBadge` from `@kairo/ui` so the look stays consistent
// with locality / decision badges elsewhere in the inspector.
//
// Status → presentation mapping:
//
//   valid          → success tone, "Valid"
//   invalid        → error tone,   "Invalid"
//   conflicted     → warn tone,    "Conflicted"
//   indeterminate  → neutral tone, "Indeterminate"
//   unverified     → neutral tone, "Unverified"
//
// `unverified` and `indeterminate` are intentionally
// indistinguishable color-wise: §10 forbids presenting either
// as `valid`, but they're also not failures — neutral matches
// that semantics. The label disambiguates them.

import type { ValidationStatus } from '@kairo/api-client';
import { validationStatusLabel } from '@kairo/object-model';
import { StatusBadge, type StatusBadgeTone } from '@kairo/ui';

export interface ValidationBadgeProps {
  status: ValidationStatus;
}

const STATUS_TO_TONE: Record<ValidationStatus, StatusBadgeTone> = {
  valid: 'success',
  invalid: 'error',
  conflicted: 'warn',
  indeterminate: 'neutral',
  unverified: 'neutral',
};

export function ValidationBadge({ status }: ValidationBadgeProps) {
  return <StatusBadge tone={STATUS_TO_TONE[status]}>{validationStatusLabel(status)}</StatusBadge>;
}
