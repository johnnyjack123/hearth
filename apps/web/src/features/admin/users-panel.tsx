import type { ServerActivity, UserActivity } from '@hearth/shared';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Activity, Ban, Ellipsis, Laptop, Plus, ShieldCheck, ShieldOff, Trash2, UserRoundCheck } from 'lucide-react';
import { type FormEvent, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { ConfirmDialog, Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { Avatar, Badge, Card, Separator, Skeleton } from '@/components/ui/display.tsx';
import { Field, Label } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/menu.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import { api } from '@/lib/api.ts';
import { authClient, isAdmin } from '@/lib/auth-client.ts';
import { usePreferences, useT } from '@/lib/preferences.tsx';
import { queryKeys, useSession } from '@/lib/queries.ts';
import { Section } from '../shell/page.tsx';

interface AdminUser {
  id: string;
  name: string;
  email: string;
  role?: string | null;
  banned?: boolean | null;
  createdAt: Date | string;
}

/** A user counts as active this long after their last request. */
const ACTIVE_MS = 5 * 60_000;

/** How often the page asks who is on the server. */
const ACTIVITY_REFRESH_MS = 15_000;

/**
 * Whether a user is on the server right now: a reply of theirs is being
 * written, or they used the web app a moment ago.
 */
function isActive(activity: UserActivity | undefined, now: number): boolean {
  return Boolean(activity && (activity.replies > 0 || (activity.lastSeenAt !== null && now - activity.lastSeenAt < ACTIVE_MS)));
}

/** "5 minutes ago", "2 hours ago" in the reader's language. */
function ago(at: number, now: number, locale: string): string {
  const format = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  const minutes = Math.round((at - now) / 60_000);
  if (minutes > -60) {
    return format.format(minutes, 'minute');
  }
  const hours = Math.round(minutes / 60);
  return hours > -24 ? format.format(hours, 'hour') : format.format(Math.round(hours / 24), 'day');
}

async function listUsers(): Promise<AdminUser[]> {
  const { data, error } = await authClient.admin.listUsers({
    query: { limit: 500, sortBy: 'createdAt', sortDirection: 'asc' },
  });
  if (error) {
    throw new Error(error.message ?? 'Could not list users');
  }
  return data.users as AdminUser[];
}

/**
 * Lists all accounts and lets admins create, promote, suspend and delete them.
 *
 * @returns The users panel.
 */
export function UsersPanel() {
  const t = useT();
  const users = useQuery({ queryKey: queryKeys.users, queryFn: listUsers });
  const activity = useQuery({ queryKey: queryKeys.activity, queryFn: api.admin.activity, refetchInterval: ACTIVITY_REFRESH_MS });
  const [creating, setCreating] = useState(false);
  const activityOf = (id: string) => activity.data?.users.find((entry) => entry.userId === id);

  return (
    <div className="flex flex-col gap-10">
      <Section title={t('admin.activity.title')} description={t('admin.activity.description')}>
        {activity.data && users.data ? <ActivityCard activity={activity.data} users={users.data} /> : <Skeleton className="h-20 w-full rounded-xl" />}
      </Section>
      <Section title={t('admin.tabUsers')} description={t('admin.usersDescription')}>
        <div className="flex justify-end">
          <Button variant="primary" onClick={() => setCreating(true)}>
            <Plus />
            {t('admin.createUser')}
          </Button>
        </div>
        <Card>
          {users.data ? (
            <ul>
              {users.data.map((user, index) => (
                <li key={user.id}>
                  {index > 0 ? <Separator /> : null}
                  <UserRow user={user} activity={activityOf(user.id)} />
                </li>
              ))}
            </ul>
          ) : (
            <div className="flex flex-col gap-3 p-4">
              <Skeleton className="h-10 w-full" />
              <Skeleton className="h-10 w-full" />
            </div>
          )}
        </Card>
        <CreateUserDialog open={creating} onOpenChange={setCreating} />
      </Section>
    </div>
  );
}

/**
 * Who is on the server right now, with running replies and connected
 * computers, so an admin knows whether an update would interrupt someone.
 */
function ActivityCard({ activity, users }: { activity: ServerActivity; users: AdminUser[] }) {
  const t = useT();
  const { locale } = usePreferences();
  const now = Date.now();
  const name = (id: string) => {
    const user = users.find((entry) => entry.id === id);
    return user ? user.name || user.email : id;
  };
  const active = activity.users.filter((entry) => isActive(entry, now));
  const replies = activity.users.reduce((sum, entry) => sum + entry.replies, 0);
  const devices = activity.users.reduce((sum, entry) => sum + entry.devices, 0);
  const since = new Date(activity.since).toLocaleString(locale, { dateStyle: 'medium', timeStyle: 'short' });
  return (
    <Card className="flex flex-col gap-2 p-4" data-activity>
      <p className="flex items-start gap-2 text-sm font-medium text-foreground">
        <Activity className={active.length > 0 ? 'mt-0.5 size-4 shrink-0 text-success' : 'mt-0.5 size-4 shrink-0 text-muted-foreground'} aria-hidden="true" />
        {active.length > 0 ? t('admin.activity.busy', { names: active.map((entry) => name(entry.userId)).join(', ') }) : t('admin.activity.idle')}
      </p>
      {replies > 0 || devices > 0 ? (
        <p className="flex flex-wrap gap-x-4 gap-y-1 text-sm text-muted-foreground">
          {replies > 0 ? <span>{t('admin.activity.replies', { count: replies })}</span> : null}
          {devices > 0 ? <span>{t('admin.activity.devices', { count: devices })}</span> : null}
        </p>
      ) : null}
      <p className="text-xs text-muted-foreground">
        {activity.users.length === 0 ? `${t('admin.activity.nobody')} ` : ''}
        {t('admin.activity.since', { date: since })}
      </p>
    </Card>
  );
}

function UserRow({ user, activity }: { user: AdminUser; activity: UserActivity | undefined }) {
  const t = useT();
  const { locale } = usePreferences();
  const now = Date.now();
  const queryClient = useQueryClient();
  const { data: session } = useSession();
  const [removing, setRemoving] = useState(false);
  const self = session?.user.id === user.id;
  const admin = isAdmin(user);

  async function run(action: () => Promise<{ error: { message?: string } | null }>, success: string) {
    const { error } = await action();
    if (error) {
      toast.error(error.message ?? t('errors.unknown'));
      throw new Error(error.message);
    }
    await queryClient.invalidateQueries({ queryKey: queryKeys.users });
    toast.success(success);
  }

  return (
    <div className="flex items-center gap-4 p-4">
      <Avatar name={user.name || user.email} />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate text-sm font-medium text-foreground">{user.name || user.email}</span>
          {self ? <Badge>{t('common.you')}</Badge> : null}
          {admin ? <Badge variant="primary">{t('account.roleAdmin')}</Badge> : null}
          {user.banned ? <Badge variant="destructive">{t('admin.banned')}</Badge> : null}
          {activity && activity.replies > 0 ? (
            <Badge variant="success">{t('admin.activity.replying')}</Badge>
          ) : isActive(activity, now) ? (
            <Badge variant="success">{t('admin.activity.active')}</Badge>
          ) : null}
          {activity && activity.devices > 0 ? (
            <Badge>
              <Laptop aria-hidden="true" />
              {t('admin.activity.devices', { count: activity.devices })}
            </Badge>
          ) : null}
        </div>
        <span className="truncate text-xs text-muted-foreground">
          {user.email} · {new Date(user.createdAt).toLocaleDateString(locale)}
          {activity?.lastSeenAt && !isActive(activity, now) ? ` · ${t('admin.activity.lastSeen', { time: ago(activity.lastSeenAt, now, locale) })}` : ''}
        </span>
      </div>
      {self ? null : (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon-sm" aria-label={t('common.moreActions')}>
              <Ellipsis />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem
              onSelect={() =>
                void run(() => authClient.admin.setRole({ userId: user.id, role: admin ? 'user' : 'admin' }), t('admin.userUpdated')).catch(
                  () => {},
                )
              }
            >
              {admin ? <ShieldOff /> : <ShieldCheck />}
              {admin ? t('admin.makeUser') : t('admin.makeAdmin')}
            </DropdownMenuItem>
            <DropdownMenuItem
              onSelect={() =>
                void run(
                  () => (user.banned ? authClient.admin.unbanUser({ userId: user.id }) : authClient.admin.banUser({ userId: user.id })),
                  t('admin.userUpdated'),
                ).catch(() => {})
              }
            >
              {user.banned ? <UserRoundCheck /> : <Ban />}
              {user.banned ? t('admin.unban') : t('admin.ban')}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem destructive onSelect={() => setRemoving(true)}>
              <Trash2 />
              {t('admin.remove')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      )}
      <ConfirmDialog
        open={removing}
        onOpenChange={setRemoving}
        title={t('admin.removeTitle')}
        description={t('admin.removeBody', { email: user.email })}
        confirmLabel={t('admin.remove')}
        destructive
        onConfirm={() => run(() => authClient.admin.removeUser({ userId: user.id }), t('admin.userRemoved'))}
      />
    </div>
  );
}

function CreateUserDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const t = useT();
  const queryClient = useQueryClient();
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [makeAdmin, setMakeAdmin] = useState(false);
  const [saving, setSaving] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setSaving(true);
    const { error } = await authClient.admin.createUser({ name, email, password, role: makeAdmin ? 'admin' : 'user' });
    setSaving(false);
    if (error) {
      toast.error(error.message ?? t('errors.unknown'));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: queryKeys.users });
    toast.success(t('admin.userCreated'));
    setName('');
    setEmail('');
    setPassword('');
    setMakeAdmin(false);
    onOpenChange(false);
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent aria-describedby={undefined}>
        <form onSubmit={submit} className="flex flex-col gap-5">
          <DialogHeader>
            <DialogTitle>{t('admin.createUserTitle')}</DialogTitle>
          </DialogHeader>
          <Field label={t('auth.name')}>
            {(control) => <Input {...control} required value={name} onChange={(event) => setName(event.target.value)} />}
          </Field>
          <Field label={t('auth.email')}>
            {(control) => (
              <Input {...control} type="email" required value={email} onChange={(event) => setEmail(event.target.value)} />
            )}
          </Field>
          <Field label={t('admin.initialPassword')} hint={t('auth.passwordHint')}>
            {(control) => (
              <Input
                {...control}
                type="password"
                autoComplete="new-password"
                required
                minLength={10}
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            )}
          </Field>
          <div className="flex items-center justify-between gap-4">
            <Label htmlFor="create-user-admin">{t('admin.makeAdmin')}</Label>
            <Switch id="create-user-admin" checked={makeAdmin} onCheckedChange={setMakeAdmin} />
          </div>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" variant="primary" loading={saving}>
              {t('common.create')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
