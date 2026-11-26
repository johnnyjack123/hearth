import { Link, Outlet, useRouterState } from '@tanstack/react-router';
import { Menu, SquarePen, WifiOff } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Logo } from '@/components/brand/logo.tsx';
import { Button } from '@/components/ui/button.tsx';
import { Sheet } from '@/components/ui/sheet.tsx';
import { serverReachable, useOffline } from '@/lib/offline.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryClient } from '@/lib/query-client.ts';
import { queryKeys } from '@/lib/queries.ts';
import { Sidebar } from './sidebar.tsx';
import { useLiveSync } from './use-live-sync.ts';

/** How often the app asks whether the server is back, while it is not. */
const RECHECK_MS = 10_000;

/**
 * Says that the app shows what it kept, and watches for the server to come
 * back; then the chat list loads again (open chats follow through the live
 * events, which reconnect on their own).
 */
function OfflineBanner() {
  const t = useT();
  const offline = useOffline();
  useEffect(() => {
    if (!offline) {
      return;
    }
    const recheck = async () => {
      if (navigator.onLine && (await serverReachable())) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.conversations, exact: true });
      }
    };
    const timer = setInterval(() => void recheck(), RECHECK_MS);
    window.addEventListener('online', recheck);
    return () => {
      clearInterval(timer);
      window.removeEventListener('online', recheck);
    };
  }, [offline]);
  if (!offline) {
    return null;
  }
  return (
    <div role="status" data-offline className="flex shrink-0 items-start gap-2 bg-warning px-4 py-2 text-sm text-warning-foreground">
      <WifiOff className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
      <span>{t('offline.banner')}</span>
    </div>
  );
}

/**
 * Layout of every signed-in page: sidebar on wide screens, a drawer plus a
 * slim header on narrow ones.
 *
 * @returns The shell with the active page in its outlet.
 */
export function AppShell() {
  const t = useT();
  const [drawerOpen, setDrawerOpen] = useState(false);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  useLiveSync();

  // Navigating from inside the drawer should close it.
  useEffect(() => {
    setDrawerOpen(false);
  }, [pathname]);

  return (
    <div className="pad-safe-area flex h-dvh overflow-hidden bg-background">
      <aside className="hidden w-(--sidebar-width) shrink-0 border-e border-sidebar-border bg-sidebar md:flex">
        <Sidebar />
      </aside>

      <Sheet
        open={drawerOpen}
        onOpenChange={setDrawerOpen}
        title={t('nav.chats')}
        className="w-(--sidebar-width) max-w-[calc(100vw-var(--spacing)*12)] bg-sidebar md:hidden"
      >
        <Sidebar />
      </Sheet>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center gap-2 border-b border-border px-2 md:hidden">
          <Button variant="ghost" size="icon" aria-label={t('nav.openMenu')} onClick={() => setDrawerOpen(true)}>
            <Menu />
          </Button>
          <Link to="/" className="flex-1 rounded-md">
            <Logo withWordmark />
          </Link>
          <Button variant="ghost" size="icon" aria-label={t('nav.newChat')} asChild>
            <Link to="/">
              <SquarePen />
            </Link>
          </Button>
        </header>
        <OfflineBanner />
        <main className="flex min-h-0 flex-1 flex-col">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
