import { Link } from '@tanstack/react-router';
import { ChevronsUpDown, LogOut, Palette, Settings, ShieldCheck } from 'lucide-react';
import { Avatar } from '@/components/ui/display.tsx';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/menu.tsx';
import { isAdmin } from '@/lib/auth-client.ts';
import { useSignOut } from '../auth/use-sign-out.ts';
import { useT } from '@/lib/preferences.tsx';
import { useSession } from '@/lib/queries.ts';

/**
 * Account button at the bottom of the sidebar with navigation to settings,
 * administration and sign-out.
 *
 * @returns The user menu.
 */
export function UserMenu() {
  const t = useT();
  const signOut = useSignOut();
  const { data: session } = useSession();
  const user = session?.user;

  if (!user) {
    return null;
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        aria-label={t('nav.userMenu')}
        className="flex w-full items-center gap-3 rounded-lg p-2 text-start transition-colors duration-(--duration-instant) hover:bg-sidebar-accent data-[state=open]:bg-sidebar-accent"
      >
        <Avatar name={user.name || user.email} src={user.image} />
        <span className="flex min-w-0 flex-1 flex-col">
          <span className="truncate text-sm font-medium text-sidebar-foreground">{user.name || user.email}</span>
          <span className="truncate text-xs text-muted-foreground">{user.email}</span>
        </span>
        <ChevronsUpDown className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
      </DropdownMenuTrigger>
      <DropdownMenuContent side="top" align="start" className="w-(--radix-dropdown-menu-trigger-width)">
        <DropdownMenuLabel className="truncate">{user.email}</DropdownMenuLabel>
        <DropdownMenuItem asChild>
          <Link to="/settings">
            <Settings />
            {t('nav.settings')}
          </Link>
        </DropdownMenuItem>
        {isAdmin(user) ? (
          <DropdownMenuItem asChild>
            <Link to="/admin">
              <ShieldCheck />
              {t('nav.admin')}
            </Link>
          </DropdownMenuItem>
        ) : null}
        {import.meta.env.DEV ? (
          <DropdownMenuItem asChild>
            <Link to="/design">
              <Palette />
              {t('nav.designSystem')}
            </Link>
          </DropdownMenuItem>
        ) : null}
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => void signOut()}>
          <LogOut />
          {t('auth.signOut')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
