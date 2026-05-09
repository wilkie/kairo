// Thin wrapper around MUI Card + CardHeader + CardContent.
// Preserves our `title / description / actions / children`
// API so call sites don't compose Card pieces themselves.

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
          titleTypographyProps={{ variant: 'h3' }}
          subheaderTypographyProps={{ variant: 'body2' }}
        />
      )}
      <CardContent>{children}</CardContent>
    </Card>
  );
}
