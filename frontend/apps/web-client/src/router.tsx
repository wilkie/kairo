// v1 route tree (`PHASE_2_WEB_CLIENT.md` slice 7):
//
//   /                         dashboard (daemon status)
//   /objects                  object list (placeholder)
//   /objects/$id              object detail
//   /actors/$id               actor detail
//   /statements/$id           statement detail
//   /blobs/$id                blob preview
//   /settings                 settings
//
// All routes mount under a common `AppShell` that draws the
// sidebar nav. Later slices fill in real content; the
// scaffolding exists so navigation, deep links, and HTML5-mode
// fallback are all wired now.

import {
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider as TanStackRouterProvider,
} from '@tanstack/react-router';
import { AppShell } from './layout/AppShell';
import { ActorDetail } from './routes/ActorDetail';
import { BlobPreviewRoute } from './routes/BlobPreview';
import { Dashboard } from './routes/Dashboard';
import { NotFound } from './routes/NotFound';
import { ObjectDetail } from './routes/ObjectDetail';
import { ObjectsList } from './routes/ObjectsList';
import { Settings } from './routes/Settings';
import { StatementDetail } from './routes/StatementDetail';

const rootRoute = createRootRoute({
  component: AppShell,
  notFoundComponent: NotFound,
});

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: Dashboard,
});

const objectsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/objects',
  component: ObjectsList,
});

const objectDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/objects/$id',
  component: () => {
    const { id } = objectDetailRoute.useParams();
    return <ObjectDetail id={id} />;
  },
});

const actorDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/actors/$id',
  component: () => {
    const { id } = actorDetailRoute.useParams();
    return <ActorDetail id={id} />;
  },
});

const statementDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/statements/$id',
  component: () => {
    const { id } = statementDetailRoute.useParams();
    return <StatementDetail id={id} />;
  },
});

const blobRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/blobs/$id',
  component: () => {
    const { id } = blobRoute.useParams();
    return <BlobPreviewRoute id={id} />;
  },
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/settings',
  component: Settings,
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  objectsRoute,
  objectDetailRoute,
  actorDetailRoute,
  statementDetailRoute,
  blobRoute,
  settingsRoute,
]);

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
