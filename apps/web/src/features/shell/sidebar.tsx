import { Link } from '@tanstack/react-router';
import { Search, SquarePen } from 'lucide-react';
import { useState } from 'react';
import { Logo } from '@/components/brand/logo.tsx';
import { Button } from '@/components/ui/button.tsx';
import { Input } from '@/components/ui/input.tsx';
import { useT } from '@/lib/preferences.tsx';
import { ConversationList } from './conversation-list.tsx';
import { UserMenu } from './user-menu.tsx';

/**
 * Sidebar content: brand, new chat, search, conversation list and user menu.
 * Rendered both in the desktop column and in the mobile drawer.
 *
 * @returns The sidebar.
 */
export function Sidebar() {
  const t = useT();
  const [query, setQuery] = useState('');

  return (
    <div className="flex h-full w-full min-w-0 flex-col text-sidebar-foreground">
      <div className="flex h-14 shrink-0 items-center px-4">
        <Link to="/" className="rounded-md">
          <Logo withWordmark />
        </Link>
      </div>

      <div className="flex shrink-0 flex-col gap-2 px-3 pb-3">
        <Button variant="outline" className="w-full justify-start" asChild>
          <Link to="/">
            <SquarePen />
            {t('nav.newChat')}
          </Link>
        </Button>
        <div className="relative">
          <Search
            className="pointer-events-none absolute start-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground"
            aria-hidden="true"
          />
          <Input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t('nav.searchChats')}
            aria-label={t('nav.searchChats')}
            className="h-8 bg-transparent ps-8 pointer-coarse:h-11"
          />
        </div>
      </div>

      <nav aria-label={t('nav.chats')} className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 pb-3">
        <ConversationList query={query} />
      </nav>

      <div className="shrink-0 border-t border-sidebar-border p-2">
        <UserMenu />
      </div>
    </div>
  );
}
