// Top-level app layout: a permanent left drawer with primary
// navigation + a main content region. The current route
// renders into `<Outlet />`. Built on MUI Drawer + List for
// idiomatic spacing, density, and active-route styling.

import { useMatch, useMatchRoute, type LinkComponentProps } from '@tanstack/react-router';
import { Link, Outlet } from '@tanstack/react-router';
import Box from '@mui/material/Box';
import Drawer from '@mui/material/Drawer';
import List from '@mui/material/List';
import ListItemButton from '@mui/material/ListItemButton';
import ListItemIcon from '@mui/material/ListItemIcon';
import ListItemText from '@mui/material/ListItemText';
import Toolbar from '@mui/material/Toolbar';
import Typography from '@mui/material/Typography';
import DashboardIcon from '@mui/icons-material/Dashboard';
import StorageIcon from '@mui/icons-material/Storage';
import SettingsIcon from '@mui/icons-material/Settings';
import { forwardRef, type ReactNode } from 'react';

type NavTo = '/' | '/objects' | '/settings';

interface NavItem {
  label: string;
  to: NavTo;
  icon: ReactNode;
  exact?: boolean;
}

const NAV_ITEMS: ReadonlyArray<NavItem> = [
  { label: 'Dashboard', to: '/', icon: <DashboardIcon />, exact: true },
  { label: 'Objects', to: '/objects', icon: <StorageIcon /> },
  { label: 'Settings', to: '/settings', icon: <SettingsIcon /> },
];

const SIDEBAR_WIDTH = 224;

export function AppShell() {
  return (
    <Box sx={{ display: 'flex', minHeight: '100vh' }}>
      <Drawer
        variant="permanent"
        sx={{
          width: SIDEBAR_WIDTH,
          flexShrink: 0,
          '& .MuiDrawer-paper': {
            width: SIDEBAR_WIDTH,
            boxSizing: 'border-box',
          },
        }}
      >
        <Toolbar sx={{ alignItems: 'center', minHeight: '64px' }}>
          <Typography variant="h6" component="h1">
            Kairo
          </Typography>
        </Toolbar>
        <List component="nav" aria-label="Primary">
          {NAV_ITEMS.map((item) => (
            <NavLinkItem key={item.to} item={item} />
          ))}
        </List>
      </Drawer>
      <Box
        component="main"
        id="main"
        sx={{
          flexGrow: 1,
          p: 4,
          maxWidth: '60rem',
          width: '100%',
          display: 'flex',
          flexDirection: 'column',
          gap: 3,
        }}
      >
        <Outlet />
      </Box>
    </Box>
  );
}

/** TanStack Router's `Link` rendered as the inner anchor of an
 * MUI ListItemButton. We bridge the two by wrapping `Link` in a
 * `forwardRef` shim so MUI's ref forwarding lands on the
 * underlying anchor. */
const RouterLink = forwardRef<HTMLAnchorElement, LinkComponentProps<'a'>>(function RouterLink(
  props,
  ref,
) {
  return <Link {...props} ref={ref} />;
});

function NavLinkItem({ item }: { item: NavItem }) {
  const matchRoute = useMatchRoute();
  const match = matchRoute({ to: item.to, fuzzy: item.exact !== true });
  const selected = match !== false;
  // `useMatch` is not used directly here, but we depend on it
  // implicitly to re-render on route change.
  useMatch({ strict: false });
  return (
    <ListItemButton
      component={RouterLink}
      to={item.to}
      selected={selected}
    >
      <ListItemIcon>{item.icon}</ListItemIcon>
      <ListItemText primary={item.label} />
    </ListItemButton>
  );
}
