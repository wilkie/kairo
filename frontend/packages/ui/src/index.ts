// `@kairo/ui` — design-system primitives for the inspector.
//
// Built on top of MUI (`@mui/material`) and Material Icons
// (`@mui/icons-material`). The wrappers in this package
// preserve a kairo-flavored prop API (variant vocabularies,
// column-driven Table, status-tone vocabulary) while delegating
// rendering, accessibility, and theming to MUI.
//
// Apps mount `<KairoUiProvider>` once near the root; that
// installs MUI's CssBaseline + the Kairo theme (light/dark via
// system preference). Material icons are imported per-call
// from `@mui/icons-material/<IconName>` for tree-shaking.

export { KairoUiProvider, type KairoUiProviderProps } from './KairoUiProvider';
export { kairoLightTheme, kairoDarkTheme, createKairoTheme, kairoBrandColors } from './theme';
export { KairoLogo, KairoIcon, type KairoLogoProps } from './KairoLogo';

export { Button, type ButtonProps, type ButtonVariant } from './Button';
export { Panel, type PanelProps } from './Panel';
export { Table, type TableProps, type TableColumn } from './Table';
export { StatusBadge, type StatusBadgeProps, type StatusBadgeTone } from './StatusBadge';
export { LocalityBadge, type LocalityBadgeProps, type LocalityState } from './LocalityBadge';
export { Tabs, type TabsProps, type TabsTab } from './Tabs';
export { Dialog, type DialogProps } from './Dialog';
export { EmptyState, type EmptyStateProps } from './EmptyState';
export { ErrorDisplay, type ErrorDisplayProps } from './ErrorDisplay';
