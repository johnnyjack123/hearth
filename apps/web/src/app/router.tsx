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
import { type EnvironmentSearch, parseEnvironmentSearch } from '@/features/chat/environment.tsx';
import { NewChatPage } from '@/features/chat/new-chat-page.tsx';
import { PairingPage } from '@/features/devices/pairing-page.tsx';
import { SETTINGS_TABS, SettingsPage, type SettingsTab } from '@/features/settings/settings-page.tsx';
import { AppShell } from '@/features/shell/app-shell.tsx';
import { NotFoundPage } from '@/features/shell/not-found-page.tsx';
import { isAdmin } from '@/lib/auth-client.ts';
import { queryClient, setUnauthorizedHandler } from '@/lib/query-client.ts';
import { queryKeys, sessionQuery } from '@/lib/queries.ts';
import { isLocalPath } from '@/lib/redirect.ts';

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

const loginRoute = createRoute({
  getParentRoute: () => authRoute,
  path: '/login',
  validateSearch: (search: Record<string, unknown>): { redirect?: string } =>
    isLocalPath(search.redirect) ? { redirect: search.redirect } : {},
  component: LoginPage,
});
const registerRoute = createRoute({ getParentRoute: () => authRoute, path: '/register', component: RegisterPage });

const appRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: 'app',
  beforeLoad: async ({ context, location }) => {
    const session = await context.queryClient.ensureQueryData(sessionQuery);
    if (!session) {
      // Come back after signing in, e.g. to confirm a device pairing.
      throw redirect({ to: '/login', search: location.href === '/' ? {} : { redirect: location.href } });
    }
    return { session };
  },
  component: AppShell,
});

const homeRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/',
  // "New chat in this environment" preselects it.
  validateSearch: (search: Record<string, unknown>): EnvironmentSearch => parseEnvironmentSearch(search),
  component: NewChatPage,
});

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

const deviceRoute = createRoute({
  getParentRoute: () => appRoute,
  path: '/device',
  validateSearch: (search: Record<string, unknown>): { code?: string } =>
    typeof search.code === 'string' && search.code.length <= 20 ? { code: search.code } : {},
  component: PairingPage,
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
  appRoute.addChildren([homeRoute, chatRoute, settingsRoute, deviceRoute, adminRoute]),
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
