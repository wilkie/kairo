// Wrapper around MUI Chip with the kairo tone vocabulary
// mapped to MUI palette colors. Color is never the only signal
// — the chip label carries the meaning so the badge is legible
// without color, satisfying the `WEB_CLIENT.md` §10/§20
// accessibility requirement.

import type { ReactElement, ReactNode } from 'react';
import Chip from '@mui/material/Chip';

export type StatusBadgeTone = 'neutral' | 'info' | 'warn' | 'error' | 'success';

export interface StatusBadgeProps {
  tone?: StatusBadgeTone;
  /** Optional leading icon. Decorative only — accessibility
   * relies on the text label. */
  icon?: ReactElement;
  children: ReactNode;
}

const TONE_TO_MUI: Record<
  StatusBadgeTone,
  { color: 'default' | 'info' | 'warning' | 'error' | 'success' }
> = {
  neutral: { color: 'default' },
  info: { color: 'info' },
  warn: { color: 'warning' },
  error: { color: 'error' },
  success: { color: 'success' },
};

export function StatusBadge({ tone = 'neutral', icon, children }: StatusBadgeProps) {
  const mui = TONE_TO_MUI[tone];
  return (
    <Chip
      size="small"
      variant="outlined"
      color={mui.color}
      label={children}
      {...(icon !== undefined ? { icon } : {})}
    />
  );
}
