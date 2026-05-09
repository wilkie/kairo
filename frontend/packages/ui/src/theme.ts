// Kairo MUI theme. Light + dark variants share the palette
// shape; dark mode is opted into via system preference at the
// `KairoUiProvider` boundary.
//
// The palette is anchored on the brand mark — magenta-purple
// (#ab00ab) and a deep teal (#007373) drawn straight out of
// the hexagonal icon. The full-saturation brand purple lives in
// the logo; for interactive surfaces we shift slightly toward
// a more legible "tooling" purple in light mode and lighten it
// for dark mode so it reads on a near-black background. Border
// radius is kept tight (4px) to echo the icon's hard angles
// without leaking the literal hexagon shape into every panel.

import { createTheme, type Theme, type ThemeOptions } from '@mui/material/styles';

/** Raw brand colors lifted from the logo SVGs. The mark uses
 * two purples: a bright icon-outline purple and a darker
 * wordmark purple. Exported so apps can use them in non-MUI
 * surfaces (charts, embeds). */
export const kairoBrandColors = {
  /** Wordmark purple — darker, used for the "Kairo" text in
   * the full logo. Also our default for interactive surfaces. */
  purpleDeep: '#780078',
  /** Icon-outline purple — brighter, used for the hexagonal
   * mark. Reserve for the logo and brand-forward accents. */
  purple: '#ab00ab',
  /** Inner-symbol teal. */
  teal: '#007373',
} as const;

const sharedShape: NonNullable<ThemeOptions['shape']> = {
  borderRadius: 4,
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
      // Wordmark purple for interactive surfaces — matches the
      // "Kairo" text in the logo. The brighter icon-outline
      // purple (`kairoBrandColors.purple`) is reserved for the
      // logo itself and brand-forward accents. Dark mode tints
      // up so the color reads on a near-black background.
      primary: {
        main: mode === 'light' ? kairoBrandColors.purpleDeep : '#d56cd5',
        contrastText: '#ffffff',
      },
      // Brand teal as the secondary/accent — pairs naturally
      // with the icon and gives us a complementary tone for
      // accents, links, and selection.
      secondary: {
        main: mode === 'light' ? kairoBrandColors.teal : '#3aa6a6',
        contrastText: '#ffffff',
      },
      background:
        mode === 'light'
          ? { default: '#f7f4f8', paper: '#ffffff' }
          : { default: '#11070f', paper: '#1c141b' },
    },
    shape: sharedShape,
    typography: sharedTypography,
    components: sharedComponents,
  });
}

export const kairoLightTheme = createKairoTheme('light');
export const kairoDarkTheme = createKairoTheme('dark');
