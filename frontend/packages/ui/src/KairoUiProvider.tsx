// Top-level UI provider: mounts MUI's CssBaseline + the Kairo
// theme (light or dark, picked from the user's system
// preference). Apps wrap their tree in `<KairoUiProvider>`
// once near the root; Storybook does the same via a decorator.
//
// Theme selection is system-driven via `useMediaQuery` so the
// inspector flips when the OS theme flips. A future setting
// can override this by adding a `mode` prop.

import { useMemo, type ReactNode } from 'react';
import CssBaseline from '@mui/material/CssBaseline';
import useMediaQuery from '@mui/material/useMediaQuery';
import { ThemeProvider } from '@mui/material/styles';
import { createKairoTheme } from './theme';

export interface KairoUiProviderProps {
  /** Force light or dark mode. Defaults to system preference. */
  mode?: 'light' | 'dark';
  children: ReactNode;
}

export function KairoUiProvider({ mode, children }: KairoUiProviderProps) {
  const prefersDark = useMediaQuery('(prefers-color-scheme: dark)', { noSsr: true });
  const resolvedMode = mode ?? (prefersDark ? 'dark' : 'light');
  const theme = useMemo(() => createKairoTheme(resolvedMode), [resolvedMode]);

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline enableColorScheme />
      {children}
    </ThemeProvider>
  );
}
