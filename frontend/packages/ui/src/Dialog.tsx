// Wrapper around MUI Dialog primitives. Preserves our
// `title / footer / children` API; MUI provides focus trap,
// ESC-to-close, and the dimmed backdrop natively.

import type { ReactNode } from 'react';
import IconButton from '@mui/material/IconButton';
import MuiDialog from '@mui/material/Dialog';
import DialogActions from '@mui/material/DialogActions';
import DialogContent from '@mui/material/DialogContent';
import DialogTitle from '@mui/material/DialogTitle';
import CloseIcon from '@mui/icons-material/Close';

export interface DialogProps {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  /** Footer typically holds confirm/cancel buttons. */
  footer?: ReactNode;
  children: ReactNode;
}

export function Dialog({ open, onClose, title, footer, children }: DialogProps) {
  return (
    <MuiDialog open={open} onClose={onClose} fullWidth maxWidth="sm">
      <DialogTitle
        sx={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: 2,
        }}
      >
        <span>{title}</span>
        <IconButton aria-label="Close dialog" onClick={onClose} size="small" edge="end">
          <CloseIcon fontSize="small" />
        </IconButton>
      </DialogTitle>
      <DialogContent dividers>{children}</DialogContent>
      {footer !== undefined && <DialogActions>{footer}</DialogActions>}
    </MuiDialog>
  );
}
