import { useQueryClient } from '@tanstack/react-query';
import { LogOut } from 'lucide-react';
import { type FormEvent, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Avatar, Badge, Card } from '@/components/ui/display.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import { authClient, isAdmin } from '@/lib/auth-client.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys, useSession } from '@/lib/queries.ts';
import { useSignOut } from '../auth/use-sign-out.ts';
import { Section } from '../shell/page.tsx';
import { CostLimitSection } from './cost-limit.tsx';
import { TwoFactorSection } from './two-factor.tsx';

/**
 * Profile, password and sign-out.
 *
 * @returns The account panel.
 */
export function AccountPanel() {
  const t = useT();
  const queryClient = useQueryClient();
  const signOut = useSignOut();
  const { data: session } = useSession();
  const user = session?.user;
  const [name, setName] = useState(user?.name ?? '');
  const [savingName, setSavingName] = useState(false);
  const [currentPassword, setCurrentPassword] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [savingPassword, setSavingPassword] = useState(false);

  if (!user) {
    return null;
  }

  async function saveName(event: FormEvent) {
    event.preventDefault();
    setSavingName(true);
    const result = await authClient.updateUser({ name: name.trim() });
    setSavingName(false);
    if (result.error) {
      toast.error(t('errors.unknown'));
      return;
    }
    await queryClient.invalidateQueries({ queryKey: queryKeys.session });
    toast.success(t('account.nameSaved'));
  }

  async function changePassword(event: FormEvent) {
    event.preventDefault();
    setSavingPassword(true);
    const result = await authClient.changePassword({ currentPassword, newPassword, revokeOtherSessions: true });
    setSavingPassword(false);
    if (result.error) {
      toast.error(result.error.code === 'INVALID_PASSWORD' ? t('auth.invalidCredentials') : t('errors.unknown'));
      return;
    }
    setCurrentPassword('');
    setNewPassword('');
    toast.success(t('account.passwordChanged'));
  }

  return (
    <div className="flex flex-col gap-10">
      <Section title={t('account.profile')}>
        <Card className="flex flex-col gap-6 p-5">
          <div className="flex items-center gap-4">
            <Avatar name={user.name || user.email} src={user.image} className="size-12 text-base" />
            <div className="flex min-w-0 flex-col gap-1">
              <span className="truncate text-sm font-medium text-foreground">{user.email}</span>
              <Badge variant={isAdmin(user) ? 'primary' : 'neutral'} className="w-fit">
                {isAdmin(user) ? t('account.roleAdmin') : t('account.roleUser')}
              </Badge>
            </div>
          </div>
          <form onSubmit={saveName} className="flex flex-col gap-3 sm:flex-row sm:items-end">
            <Field label={t('account.name')} className="flex-1">
              {(control) => <Input {...control} required value={name} onChange={(event) => setName(event.target.value)} />}
            </Field>
            <Button type="submit" variant="secondary" loading={savingName} disabled={!name.trim() || name.trim() === user.name}>
              {t('account.saveName')}
            </Button>
          </form>
        </Card>
      </Section>

      <Section title={t('account.changePassword')}>
        <Card className="p-5">
          <form onSubmit={changePassword} className="flex flex-col gap-4">
            <Field label={t('account.currentPassword')}>
              {(control) => (
                <Input
                  {...control}
                  type="password"
                  autoComplete="current-password"
                  required
                  value={currentPassword}
                  onChange={(event) => setCurrentPassword(event.target.value)}
                />
              )}
            </Field>
            <Field label={t('account.newPassword')} hint={t('auth.passwordHint')}>
              {(control) => (
                <Input
                  {...control}
                  type="password"
                  autoComplete="new-password"
                  required
                  minLength={10}
                  value={newPassword}
                  onChange={(event) => setNewPassword(event.target.value)}
                />
              )}
            </Field>
            <div>
              <Button type="submit" variant="secondary" loading={savingPassword}>
                {t('account.changePassword')}
              </Button>
            </div>
          </form>
        </Card>
      </Section>

      <TwoFactorSection />

      <CostLimitSection />

      <div>
        <Button variant="outline" onClick={() => void signOut()}>
          <LogOut />
          {t('auth.signOut')}
        </Button>
      </div>
    </div>
  );
}
