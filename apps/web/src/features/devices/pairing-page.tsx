import type { PairingInfo } from '@harness/shared';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Link, useNavigate, useSearch } from '@tanstack/react-router';
import { CircleAlert, CircleCheck, CircleX, Clock, Globe, Laptop, ShieldAlert } from 'lucide-react';
import { type FormEvent, type ReactNode, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Card, Skeleton } from '@/components/ui/display.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import { ApiError, api, errorMessage } from '@/lib/api.ts';
import { usePreferences, useT } from '@/lib/preferences.tsx';
import { queryKeys } from '@/lib/queries.ts';
import { Page } from '../shell/page.tsx';

/**
 * Confirmation page for `harness login` (`/device?code=ABCD-EFGH`): shows
 * which device asks for access and lets the user connect or deny it.
 *
 * @returns The pairing page.
 */
export function PairingPage() {
  const t = useT();
  const { code } = useSearch({ strict: false }) as { code?: string };
  const pairing = useQuery({
    queryKey: ['pairing', code],
    queryFn: () => api.devices.pairing(code ?? ''),
    enabled: Boolean(code),
    retry: false,
  });
  const notFound = pairing.error instanceof ApiError && pairing.error.status === 404;

  return (
    <Page title={t('pairing.title')} description={t('pairing.description')}>
      <Card className="flex max-w-xl flex-col gap-6 p-6">
        {!code || notFound ? (
          <CodeForm initial={code} notFound={notFound} />
        ) : pairing.data ? (
          <Decision info={pairing.data} code={code} />
        ) : pairing.error ? (
          <p className="text-sm text-destructive">{errorMessage(t, pairing.error)}</p>
        ) : (
          <div className="flex flex-col gap-3">
            <Skeleton className="h-8 w-40" />
            <Skeleton className="h-16 w-full" />
          </div>
        )}
      </Card>
    </Page>
  );
}

function CodeForm({ initial, notFound }: { initial?: string; notFound: boolean }) {
  const t = useT();
  const navigate = useNavigate();
  const [value, setValue] = useState(initial ?? '');

  function submit(event: FormEvent) {
    event.preventDefault();
    void navigate({ to: '/device', search: { code: value.trim().toUpperCase() } });
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-4">
      <Field label={t('pairing.code')} error={notFound ? t('pairing.notFound') : undefined}>
        {(control) => (
          <Input
            {...control}
            required
            autoFocus
            autoComplete="off"
            placeholder="ABCD-EFGH"
            className="font-mono uppercase tracking-widest"
            value={value}
            onChange={(event) => setValue(event.target.value)}
          />
        )}
      </Field>
      <Button type="submit" variant="primary" className="self-start">
        {t('pairing.lookup')}
      </Button>
    </form>
  );
}

/**
 * Where the request came from. A different address than the browser's is
 * normal when the computer is elsewhere, but it is also what a stranger's
 * pairing looks like when its link was sent to someone.
 */
function Origin({ info }: { info: PairingInfo }) {
  const { t, locale } = usePreferences();
  if (info.ip === null) {
    return null;
  }
  const time = new Date(info.createdAt).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' });
  return (
    <>
      <p className="flex items-start gap-2 text-sm text-muted-foreground">
        <Globe className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
        <span>
          {t('pairing.origin', { ip: info.ip, time })}
          {info.sameAddress ? ` ${t('pairing.sameAddress')}` : null}
        </span>
      </p>
      {info.sameAddress === false ? (
        <p role="alert" className="flex items-start gap-2 rounded-lg bg-destructive-subtle p-3 text-sm text-destructive">
          <CircleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
          {t('pairing.otherAddress')}
        </p>
      ) : null}
    </>
  );
}

function Decision({ info, code }: { info: PairingInfo; code: string }) {
  const t = useT();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState<'approve' | 'deny' | null>(null);

  async function decide(approve: boolean) {
    setBusy(approve ? 'approve' : 'deny');
    try {
      const next = await api.devices.decide(code, approve);
      queryClient.setQueryData(['pairing', code], next);
      await queryClient.invalidateQueries({ queryKey: queryKeys.devices });
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setBusy(null);
    }
  }

  const header = (
    <div className="flex items-center gap-4">
      <div className="flex size-12 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
        <Laptop className="size-6" aria-hidden="true" />
      </div>
      <div className="flex min-w-0 flex-col gap-1">
        <span className="text-sm font-medium text-foreground">{t('pairing.request', { name: info.name })}</span>
        <span className="font-mono text-2xl font-semibold tracking-widest text-foreground">{info.userCode}</span>
      </div>
    </div>
  );

  switch (info.status) {
    case 'pending':
      return (
        <>
          {header}
          <Origin info={info} />
          <p className="text-sm text-muted-foreground">{t('pairing.explain')}</p>
          <p className="flex items-start gap-2 text-sm text-foreground">
            <ShieldAlert className="mt-0.5 size-4 shrink-0 text-warning" aria-hidden="true" />
            {t('pairing.compare')}
          </p>
          <div className="flex flex-wrap gap-3">
            <Button variant="primary" loading={busy === 'approve'} disabled={busy !== null} onClick={() => void decide(true)}>
              {t('pairing.approve')}
            </Button>
            <Button variant="outline" loading={busy === 'deny'} disabled={busy !== null} onClick={() => void decide(false)}>
              {t('pairing.deny')}
            </Button>
          </div>
        </>
      );
    case 'approved':
      return (
        <Outcome icon={<CircleCheck className="size-5 text-success" aria-hidden="true" />} header={header}>
          {t('pairing.approved')}{' '}
          <Link to="/settings" search={{ tab: 'devices' }} className="font-medium text-primary underline-offset-4 hover:underline">
            {t('pairing.manage')}
          </Link>
        </Outcome>
      );
    case 'denied':
      return (
        <Outcome icon={<CircleX className="size-5 text-destructive" aria-hidden="true" />} header={header}>
          {t('pairing.denied')}
        </Outcome>
      );
    case 'expired':
      return (
        <Outcome icon={<Clock className="size-5 text-muted-foreground" aria-hidden="true" />} header={header}>
          {t('pairing.expired')}
        </Outcome>
      );
  }
}

function Outcome({ icon, header, children }: { icon: ReactNode; header: ReactNode; children: ReactNode }) {
  return (
    <>
      {header}
      <p role="status" className="flex items-start gap-2 text-sm text-foreground">
        <span className="mt-0.5 shrink-0">{icon}</span>
        <span>{children}</span>
      </p>
    </>
  );
}
