import type { ButtonHTMLAttributes, ReactNode } from 'react';

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  children: ReactNode;
};

/**
 * Slice 5 placeholder. Slice 7 replaces this with the real
 * design-system Button (variants, sizes, icon slots, focus ring,
 * keyboard parity per `WEB_CLIENT.md` §20). For now: a typed,
 * pass-through native button so the package has something
 * exporting and the workspace's import graph compiles.
 */
export function Button({ children, ...rest }: ButtonProps) {
  return <button {...rest}>{children}</button>;
}
