// Locality badge — surfaces where data lives + how reachable
// it is, deliberately separate from validity (see
// `specs/WEB_CLIENT.md` §15). v1 only ever emits `local`
// because there's no federation yet, but the full vocabulary
// is implemented now so federation can land later without a
// rewrite.
//
// Renders as a `StatusBadge` so the label text always carries
// the meaning — accessibility (`WEB_CLIENT.md` §10/§20) does
// not rely on color alone.

import { StatusBadge, type StatusBadgeTone } from './StatusBadge';

export type LocalityState =
  | 'local'
  | 'remote'
  | 'cached'
  | 'pinned'
  | 'partial'
  | 'missing'
  | 'fetching'
  | 'unavailable';

export interface LocalityBadgeProps {
  state: LocalityState;
}

const STATE_TO_PRESENTATION: Record<
  LocalityState,
  { tone: StatusBadgeTone; label: string }
> = {
  local: { tone: 'success', label: 'Local' },
  remote: { tone: 'info', label: 'Remote' },
  cached: { tone: 'info', label: 'Cached' },
  pinned: { tone: 'success', label: 'Pinned' },
  partial: { tone: 'warn', label: 'Partial' },
  missing: { tone: 'error', label: 'Missing' },
  fetching: { tone: 'info', label: 'Fetching' },
  unavailable: { tone: 'error', label: 'Unavailable' },
};

export function LocalityBadge({ state }: LocalityBadgeProps) {
  const { tone, label } = STATE_TO_PRESENTATION[state];
  return <StatusBadge tone={tone}>{label}</StatusBadge>;
}
