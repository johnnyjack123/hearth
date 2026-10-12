import type { QueryClient } from '@tanstack/react-query';
import {
  createRootRouteWithContext,
  createRoute,
  createRouter,
  lazyRouteComponent,
  Outlet,
  redirect,
} from '@tanstack/react-router';
import { AdminPage } from '@/features/admin/admin-page.tsx';
import { AuthLayout } from '@/features/auth/auth-layout.tsx';
import { LoginPage, RegisterPage } from '@/features/auth/auth-pages.tsx';
import { ChatPage } from '@/features/chat/chat-page.tsx';
import { disposeAllChats } from '@/features/chat/chat-store.ts';
import { NewChatPage } from '@/features/chat/new-chat-page.tsx';
import { SETTINGS_TABS, SettingsPage, type SettingsTab } from '@/features/settings/settings-page.tsx';
import { AppShell } from '@/features/shell/app-shell.tsx';
import { NotFoundPage } from '@/features/shell/not-found-page.tsx';
import { isAdmin } from '@/lib/auth-client.ts';
import { queryClient, setUnauthorizedHandler } from '@/lib/query-client.ts';
import { queryKeys, sessionQuery } from '@/lib/queries.ts';

interface RouterContext {
  queryClient: QueryClient;
}

const rootRoute = createRootRouteWithContext<RouterContext>()({
  component: Outlet,
  notFoundComponent: NotFoundPage,
});

const authRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: 'auth',
  beforeLoad: async ({ context }) => {
    if (await context.queryClient.ensureQueryData(sessionQuery)) {
      throw redirect({ to: '/' });
    }
  },
  component: AuthLayout,
});

const loginRoute = createRoute({ getParentRoute: () => authRoute, path: '/login', component: LoginPage });
const registerRoute = createRoute({ getParentRoute: () => authRoute, path: '/register', component: RegisterPage });

const appRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: 'app',
  beforeLoad: async ({ context }) => {
    const session = await context.queryClient.ensureQueryData(sessionQuery);
    if (!session) {
      throw redirect({ to: '/login' });
    }
    return { session };
  },
  component: AppShell,
});

const homeRoute = createRoute({ getParentRoute: () => appRoute, path: '/', component: NewChatPage });

const chatRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/c/$conversationId',
  component: ChatPage,
});

const settingsRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/settings',
  validateSearch: (search: Record<string, unknown>): { tab?: SettingsTab } =>
    SETTINGS_TABS.includes(search.tab as SettingsTab) ? { tab: search.tab as SettingsTab } : {},
  component: SettingsPage,
});

const adminRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/admin',
  beforeLoad: ({ context }) => {
    if (!isAdmin(context.session.user)) {
      throw redirect({ to: '/' });
    }
  },
  component: AdminPage,
});

// Development-only component gallery; the dynamic import is dropped from production builds.
const designRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/design',
  component: import.meta.env.DEV
    ? lazyRouteComponent(() => import('@/features/design/design-page.tsx'), 'DesignPage')
    : NotFoundPage,
});

const routeTree = rootRoute.addChildren([
  authRoute.addChildren([loginRoute, registerRoute]),
  appRoute.addChildren([homeRoute, chatRoute, settingsRoute, adminRoute]),
  designRoute,
]);

/**
 * The application router.
 */
export const router = createRouter({
  routeTree,
  context: { queryClient },
  defaultPreload: 'intent',
  scrollRestoration: true,
});

setUnauthorizedHandler(() => {
  disposeAllChats();
  queryClient.setQueryData(queryKeys.session, null);
  void router.navigate({ to: '/login' });
});

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router;
  }
}
