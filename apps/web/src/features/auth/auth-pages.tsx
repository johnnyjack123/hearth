import { useQueryClient } from '@tanstack/react-query';
import { Link, Navigate, useNavigate, useSearch } from '@tanstack/react-router';
import { CircleAlert, Sparkles } from 'lucide-react';
import { type FormEvent, type ReactNode, useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import type { MessageKey } from '@/i18n/index.ts';
import { authClient } from '@/lib/auth-client.ts';
import { clearOfflineData } from '@/lib/offline.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys, sessionQuery, usePublicSettings } from '@/lib/queries.ts';
import { isLocalPath } from '@/lib/redirect.ts';

// Better Auth error codes we explain in our own words; anything else is generic.
const AUTH_ERRORS: Record<string, MessageKey> = {
  INVALID_EMAIL_OR_PASSWORD: 'auth.invalidCredentials',
  REGISTRATION_CLOSED: 'auth.registrationClosed',
  BANNED_USER: 'auth.banned',
  SETUP_CODE_INVALID: 'auth.setupCodeInvalid',
  TOO_MANY_REQUESTS: 'auth.tooManyRequests',
  INVALID_CODE: 'auth.twoFactor.invalidCode',
  INVALID_BACKUP_CODE: 'auth.twoFactor.invalidBackupCode',
  ACCOUNT_TEMPORARILY_LOCKED: 'auth.twoFactor.locked',
  INVALID_TWO_FACTOR_COOKIE: 'auth.twoFactor.expired',
};

// Better Auth's own rate limit answers 429 without a code.
function authErrorKey(code: string | undefined, status?: number): MessageKey {
  return (code && AUTH_ERRORS[code]) || (status === 429 ? 'auth.tooManyRequests' : 'errors.unknown');
}

function FormHeader({ title, subtitle }: { title: string; subtitle: string }) {
  return (
    <div className="flex flex-col gap-1.5">
      <h1 className="text-2xl font-semibold tracking-tight text-foreground">{title}</h1>
      <p className="text-sm text-muted-foreground">{subtitle}</p>
    </div>
  );
}

function FormError({ children }: { children: ReactNode }) {
  return (
    <p role="alert" className="flex items-start gap-2 rounded-lg bg-destructive-subtle p-3 text-sm text-destructive">
      <CircleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
      {children}
    </p>
  );
}

function useAfterAuth() {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { redirect } = useSearch({ strict: false }) as { redirect?: string };
  return async () => {
    // Another account may have left chats in the offline cache.
    await clearOfflineData();
    // Fetch, don't just invalidate: nothing observes the session on the auth pages,
    // so an invalidated query would not refetch and the route guard would still read
    // the cached "signed out" value and bounce back to /login.
    await queryClient.fetchQuery({ ...sessionQuery, staleTime: 0 });
    await queryClient.invalidateQueries({ queryKey: queryKeys.publicSettings });
    // Back to where the sign-in was required, e.g. a device pairing.
    await (isLocalPath(redirect) ? navigate({ href: redirect }) : navigate({ to: '/' }));
  };
}

/**
 * Sign-in form (`/login`).
 *
 * @returns The login page.
 */
export function LoginPage() {
  const t = useT();
  const afterAuth = useAfterAuth();
  const settings = usePublicSettings();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<MessageKey | null>(null);
  const [busy, setBusy] = useState(false);
  // The password was right and the account has two-factor sign-in on.
  const [needsCode, setNeedsCode] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const result = await authClient.signIn.email({ email, password });
      if (result.error) {
        setError(authErrorKey(result.error.code, result.error.status));
        return;
      }
      if ('twoFactorRedirect' in result.data && result.data.twoFactorRedirect) {
        setNeedsCode(true);
        return;
      }
      await afterAuth();
    } catch {
      setError('errors.network');
    } finally {
      setBusy(false);
    }
  }

  const canRegister = settings.data?.registration === 'open' || settings.data?.hasUsers === false;

  // A fresh server has nobody to sign in; send the first person to registration.
  if (settings.data?.hasUsers === false) {
    return <Navigate to="/register" replace />;
  }

  if (needsCode) {
    return (
      <CodeStep
        onDone={afterAuth}
        onExpired={() => {
          setNeedsCode(false);
          setError('auth.twoFactor.expired');
        }}
      />
    );
  }

  return (
    <>
      <FormHeader title={t('auth.loginTitle')} subtitle={t('auth.loginSubtitle')} />
      <form onSubmit={submit} className="flex flex-col gap-5">
        {error ? <FormError>{t(error)}</FormError> : null}
        <Field label={t('auth.email')}>
          {(control) => (
            <Input {...control} type="email" autoComplete="email" required value={email} onChange={(event) => setEmail(event.target.value)} />
          )}
        </Field>
        <Field label={t('auth.password')}>
          {(control) => (
            <Input
              {...control}
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(event) => setPassword(event.target.value)}
            />
          )}
        </Field>
        <Button type="submit" variant="primary" size="lg" loading={busy}>
          {t('auth.signIn')}
        </Button>
      </form>
      {canRegister ? (
        <p className="text-center text-sm text-muted-foreground">
          {t('auth.noAccount')}{' '}
          <Link to="/register" className="font-medium text-primary underline-offset-4 hover:underline">
            {t('auth.toRegister')}
          </Link>
        </p>
      ) : null}
    </>
  );
}

/**
 * The second step of a sign-in with two-factor on: the code of the
 * authenticator app, or one of the backup codes.
 */
function CodeStep({ onDone, onExpired }: { onDone: () => Promise<void>; onExpired: () => void }) {
  const t = useT();
  const [backup, setBackup] = useState(false);
  const [code, setCode] = useState('');
  const [trust, setTrust] = useState(false);
  const [error, setError] = useState<MessageKey | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const input = { code: backup ? code.trim() : code.replace(/\s/g, ''), trustDevice: trust };
      const result = backup ? await authClient.twoFactor.verifyBackupCode(input) : await authClient.twoFactor.verifyTotp(input);
      if (result.error) {
        // The password step is minutes old; start over.
        if (result.error.code === 'INVALID_TWO_FACTOR_COOKIE') {
          onExpired();
          return;
        }
        setError(authErrorKey(result.error.code, result.error.status));
        return;
      }
      await onDone();
    } catch {
      setError('errors.network');
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <FormHeader title={t('auth.twoFactor.title')} subtitle={backup ? t('auth.twoFactor.backupSubtitle') : t('auth.twoFactor.subtitle')} />
      <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-5">
        {error ? <FormError>{t(error)}</FormError> : null}
        <Field label={backup ? t('auth.twoFactor.backupCode') : t('auth.twoFactor.code')}>
          {(control) => (
            <Input
              {...control}
              required
              autoFocus
              autoComplete="one-time-code"
              inputMode={backup ? 'text' : 'numeric'}
              className="font-mono tracking-widest"
              value={code}
              onChange={(event) => setCode(event.target.value)}
            />
          )}
        </Field>
        <label className="flex cursor-pointer items-center gap-3 text-sm text-foreground">
          <Switch checked={trust} onCheckedChange={setTrust} aria-label={t('auth.twoFactor.trust')} />
          {t('auth.twoFactor.trust')}
        </label>
        <Button type="submit" variant="primary" size="lg" loading={busy}>
          {t('auth.twoFactor.verify')}
        </Button>
      </form>
      <Button
        variant="link"
        className="self-center"
        onClick={() => {
          setBackup(!backup);
          setCode('');
          setError(null);
        }}
      >
        {backup ? t('auth.twoFactor.useApp') : t('auth.twoFactor.useBackup')}
      </Button>
    </>
  );
}

/**
 * Registration form (`/register`). Explains that the first account becomes
 * admin, and shows a notice instead of the form when registration is closed.
 *
 * @returns The registration page.
 */
export function RegisterPage() {
  const t = useT();
  const afterAuth = useAfterAuth();
  const settings = usePublicSettings();
  const [name, setName] = useState('');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [setupCode, setSetupCode] = useState('');
  const [error, setError] = useState<MessageKey | null>(null);
  const [busy, setBusy] = useState(false);

  const closed = settings.data?.registration === 'closed' && settings.data.hasUsers;
  const firstUser = settings.data?.hasUsers === false;
  const needsCode = firstUser && settings.data?.setupCode === true;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const result = await authClient.signUp.email({
        name,
        email,
        password,
        // The server checks it only for the first account.
        ...(needsCode ? { fetchOptions: { headers: { 'x-hearth-setup-code': setupCode } } } : {}),
      });
      if (result.error) {
        setError(authErrorKey(result.error.code, result.error.status));
        return;
      }
      await afterAuth();
    } catch {
      setError('errors.network');
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <FormHeader title={t('auth.registerTitle')} subtitle={t('auth.registerSubtitle')} />
      {closed ? (
        <FormError>{t('auth.registrationClosed')}</FormError>
      ) : (
        <form onSubmit={submit} className="flex flex-col gap-5">
          {firstUser ? (
            <p className="flex items-start gap-2 rounded-lg bg-primary/10 p-3 text-sm text-foreground">
              <Sparkles className="mt-0.5 size-4 shrink-0 text-primary" aria-hidden="true" />
              {t('auth.firstUserHint')}
            </p>
          ) : null}
          {error ? <FormError>{t(error)}</FormError> : null}
          {needsCode ? (
            <Field label={t('auth.setupCode')} hint={t('auth.setupCodeHint')}>
              {(control) => (
                <Input
                  {...control}
                  required
                  autoComplete="off"
                  placeholder="ABCD-EFGH"
                  className="font-mono uppercase tracking-widest"
                  value={setupCode}
                  onChange={(event) => setSetupCode(event.target.value)}
                />
              )}
            </Field>
          ) : null}
          <Field label={t('auth.name')}>
            {(control) => <Input {...control} autoComplete="name" required value={name} onChange={(event) => setName(event.target.value)} />}
          </Field>
          <Field label={t('auth.email')}>
            {(control) => (
              <Input {...control} type="email" autoComplete="email" required value={email} onChange={(event) => setEmail(event.target.value)} />
            )}
          </Field>
          <Field label={t('auth.password')} hint={t('auth.passwordHint')}>
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
          <Button type="submit" variant="primary" size="lg" loading={busy}>
            {t('auth.signUp')}
          </Button>
        </form>
      )}
      <p className="text-center text-sm text-muted-foreground">
        {t('auth.haveAccount')}{' '}
        <Link to="/login" className="font-medium text-primary underline-offset-4 hover:underline">
          {t('auth.toLogin')}
        </Link>
      </p>
    </>
  );
}
