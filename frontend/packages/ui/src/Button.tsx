// Thin wrapper around MUI Button. We keep our own variant
// vocabulary (`default | primary | ghost`) so call sites read
// in kairo-flavored terms; under the hood it maps to MUI's
// `variant` + `color` props.

import MuiButton, { type ButtonProps as MuiButtonProps } from '@mui/material/Button';

export type ButtonVariant = 'default' | 'primary' | 'ghost';

export interface ButtonProps extends Omit<MuiButtonProps, 'variant' | 'color'> {
  variant?: ButtonVariant;
}

/**
 * Default variant: outlined neutral button.
 * Primary: filled primary-color button (the headline action).
 * Ghost: text-only button for low-weight actions.
 */
export function Button({ variant = 'default', ...rest }: ButtonProps) {
  switch (variant) {
    case 'primary':
      return <MuiButton variant="contained" color="primary" {...rest} />;
    case 'ghost':
      return <MuiButton variant="text" color="inherit" {...rest} />;
    case 'default':
    default:
      return <MuiButton variant="outlined" color="inherit" {...rest} />;
  }
}
