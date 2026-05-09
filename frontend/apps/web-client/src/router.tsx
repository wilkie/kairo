// Slice 5 router shell. Slice 7 lands the full route tree
// (`/`, `/objects/$id`, `/actors/$id`, `/statements/$id`,
// `/blobs/$id`, `/settings`). For now there's a single index
// route that renders the App component.

import {
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider as TanStackRouterProvider,
} from '@tanstack/react-router';
import { App } from './App';

const rootRoute = createRootRoute({
  component: () => <Outlet />,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: App,
});

const routeTree = rootRoute.addChildren([indexRoute]);

export const router = createRouter({
  routeTree,
  defaultPreload: 'intent',
});

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}

export function RouterProvider() {
  return <TanStackRouterProvider router={router} />;
}
