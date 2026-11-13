import { Link, Outlet, useRouterState } from '@tanstack/react-router';
import { Menu, SquarePen } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Logo } from '@/components/brand/logo.tsx';
import { Button } from '@/components/ui/button.tsx';
import { Sheet } from '@/components/ui/sheet.tsx';
import { useT } from '@/lib/preferences.tsx';
import { Sidebar } from './sidebar.tsx';

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
        <main className="flex min-h-0 flex-1 flex-col">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
