import { useQueryClient } from '@tanstack/react-query';
import { Link, Navigate, useNavigate, useSearch } from '@tanstack/react-router';
import { CircleAlert, Sparkles } from 'lucide-react';
import { type FormEvent, type ReactNode, useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import type { MessageKey } from '@/i18n/index.ts';
import { authClient } from '@/lib/auth-client.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys, sessionQuery, usePublicSettings } from '@/lib/queries.ts';
import { isLocalPath } from '@/lib/redirect.ts';

// Better Auth error codes we explain in our own words; anything else is generic.
const AUTH_ERRORS: Record<string, MessageKey> = {
  INVALID_EMAIL_OR_PASSWORD: 'auth.invalidCredentials',
  REGISTRATION_CLOSED: 'auth.registrationClosed',
  BANNED_USER: 'auth.banned',
};

function authErrorKey(code: string | undefined): MessageKey {
  return (code && AUTH_ERRORS[code]) || 'errors.unknown';
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

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const result = await authClient.signIn.email({ email, password });
      if (result.error) {
        setError(authErrorKey(result.error.code));
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
  const [error, setError] = useState<MessageKey | null>(null);
  const [busy, setBusy] = useState(false);

  const closed = settings.data?.registration === 'closed' && settings.data.hasUsers;
  const firstUser = settings.data?.hasUsers === false;

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const result = await authClient.signUp.email({ name, email, password });
      if (result.error) {
        setError(authErrorKey(result.error.code));
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
