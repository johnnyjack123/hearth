import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Ban, Ellipsis, Plus, ShieldCheck, ShieldOff, Trash2, UserRoundCheck } from 'lucide-react';
import { type FormEvent, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { ConfirmDialog, Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { Avatar, Badge, Card, Separator, Skeleton } from '@/components/ui/display.tsx';
import { Field, Label } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from '@/components/ui/menu.tsx';
import { Switch } from '@/components/ui/switch.tsx';
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
  const [creating, setCreating] = useState(false);

  return (
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
                <UserRow user={user} />
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
  );
}

function UserRow({ user }: { user: AdminUser }) {
  const t = useT();
  const { locale } = usePreferences();
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
        </div>
        <span className="truncate text-xs text-muted-foreground">
          {user.email} · {new Date(user.createdAt).toLocaleDateString(locale)}
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
