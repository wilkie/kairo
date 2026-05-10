// Thin wrapper around MUI Card + CardHeader + CardContent.
// Preserves our `title / description / actions / children`
// API so call sites don't compose Card pieces themselves.
//
// `titleTypographyProps.component='h3'` is set explicitly:
// MUI's CardHeader otherwise wraps the title in a `<span>`,
// even when the typography variant is `'h3'` — that produces
// the right *style* but the wrong *element*, breaking heading
// structure for screen readers (and for `getByRole('heading')`
// tests). The page title stays at h2 (set per route); panel
// titles sit underneath at h3.

import { type ReactNode } from 'react';
import Card from '@mui/material/Card';
import CardContent from '@mui/material/CardContent';
import CardHeader from '@mui/material/CardHeader';

export interface PanelProps {
  title?: ReactNode;
  description?: ReactNode;
  /** Right-side content in the panel header (actions). */
  actions?: ReactNode;
  children: ReactNode;
}

/**
 * Card-like container with an optional header (title +
 * description + actions) and a padded body. Used as the
 * baseline section grouping in the inspector.
 */
export function Panel({ title, description, actions, children }: PanelProps) {
  const hasHeader = title !== undefined || description !== undefined || actions !== undefined;
  return (
    <Card>
      {hasHeader && (
        <CardHeader
          title={title}
          subheader={description}
          action={actions}
          titleTypographyProps={{ variant: 'h3', component: 'h3' }}
          subheaderTypographyProps={{ variant: 'body2' }}
        />
      )}
      <CardContent>{children}</CardContent>
    </Card>
  );
}
