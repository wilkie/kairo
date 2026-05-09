// Kairo MUI theme. Light + dark variants share the palette
// shape; dark mode is opted into via system preference at the
// `KairoUiProvider` boundary.
//
// We keep the palette deliberately close to MUI's defaults for
// most semantic colors so out-of-the-box components (Alert,
// Chip, Button) feel native. The accent moves to a slightly
// less-saturated indigo than MUI's default to read as "tooling"
// rather than "marketing."

import { createTheme, type Theme, type ThemeOptions } from '@mui/material/styles';

const sharedShape: NonNullable<ThemeOptions['shape']> = {
  borderRadius: 6,
};

const sharedTypography: NonNullable<ThemeOptions['typography']> = {
  fontFamily: [
    'system-ui',
    '-apple-system',
    'Segoe UI',
    'Roboto',
    'Helvetica',
    'Arial',
    'sans-serif',
  ].join(','),
  // Slightly tighter line-height than MUI's defaults — the
  // inspector renders dense metadata rows.
  body1: { fontSize: '0.9375rem' },
  body2: { fontSize: '0.875rem' },
  // Page titles
  h1: { fontSize: '1.75rem', fontWeight: 600 },
  h2: { fontSize: '1.5rem', fontWeight: 600 },
  h3: { fontSize: '1.125rem', fontWeight: 600 },
};

const sharedComponents: NonNullable<ThemeOptions['components']> = {
  MuiButton: {
    defaultProps: {
      // Avoid MUI's default uppercase shouting on every button.
      disableElevation: true,
    },
    styleOverrides: {
      root: { textTransform: 'none' },
    },
  },
  MuiCard: {
    defaultProps: {
      variant: 'outlined',
    },
  },
  MuiTextField: {
    defaultProps: {
      variant: 'outlined',
      size: 'small',
    },
  },
  MuiTable: {
    defaultProps: {
      size: 'small',
    },
  },
  MuiTableCell: {
    styleOverrides: {
      head: {
        fontWeight: 600,
        textTransform: 'uppercase',
        letterSpacing: '0.04em',
        fontSize: '0.75rem',
      },
    },
  },
};

export function createKairoTheme(mode: 'light' | 'dark'): Theme {
  return createTheme({
    palette: {
      mode,
      primary: {
        main: mode === 'light' ? '#3a6df0' : '#6191ff',
      },
      background:
        mode === 'light'
          ? { default: '#f6f7f9', paper: '#ffffff' }
          : { default: '#0f1115', paper: '#1a1f26' },
    },
    shape: sharedShape,
    typography: sharedTypography,
    components: sharedComponents,
  });
}

export const kairoLightTheme = createKairoTheme('light');
export const kairoDarkTheme = createKairoTheme('dark');
