import { useQueryClient } from '@tanstack/react-query';
import { Copy, ShieldCheck } from 'lucide-react';
import { type FormEvent, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { Badge, Card } from '@/components/ui/display.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import { QrCode } from '@/components/ui/qr-code.tsx';
import type { MessageKey } from '@/i18n/index.ts';
import { authClient } from '@/lib/auth-client.ts';
import { useT } from '@/lib/preferences.tsx';
import { sessionQuery, useSession } from '@/lib/queries.ts';
import { Section } from '../shell/page.tsx';

/** What a two-factor request failed with, in the user's words. */
function failure(error: { code?: string; status: number }): MessageKey {
  if (error.status === 429) {
    return 'auth.tooManyRequests';
  }
  if (error.code === 'INVALID_PASSWORD') {
    return 'twoFactor.wrongPassword';
  }
  if (error.code === 'INVALID_CODE') {
    return 'twoFactor.wrongCode';
  }
  return 'errors.unknown';
}

/** The secret of an otpauth URI in groups of four, for typing it into an app by hand. */
function secretOf(uri: string): string {
  return (new URL(uri).searchParams.get('secret') ?? '').replace(/(.{4})/g, '$1 ').trim();
}

type Dialogs = 'enable' | 'disable' | 'codes' | null;

/**
 * Two-factor sign-in in the account settings: set up with an authenticator
 * app, new backup codes, or off again. Every step asks for the password.
 *
 * @returns The section.
 */
export function TwoFactorSection() {
  const t = useT();
  const { data: session } = useSession();
  const [dialog, setDialog] = useState<Dialogs>(null);
  const enabled = session?.user.twoFactorEnabled === true;

  return (
    <Section title={t('twoFactor.title')} description={t('twoFactor.description')}>
      <Card className="flex flex-col gap-4 p-5">
        <div className="flex items-center gap-3">
          <ShieldCheck className={enabled ? 'size-5 text-success' : 'size-5 text-muted-foreground'} aria-hidden="true" />
          <span className="flex-1 text-sm text-foreground">{enabled ? t('twoFactor.on') : t('twoFactor.off')}</span>
          {enabled ? <Badge variant="success">{t('twoFactor.active')}</Badge> : null}
        </div>
        <div className="flex flex-wrap gap-3">
          {enabled ? (
            <>
              <Button variant="secondary" onClick={() => setDialog('codes')}>
                {t('twoFactor.newCodes')}
              </Button>
              <Button variant="outline" onClick={() => setDialog('disable')}>
                {t('twoFactor.disable')}
              </Button>
            </>
          ) : (
            <Button variant="primary" onClick={() => setDialog('enable')}>
              {t('twoFactor.enable')}
            </Button>
          )}
        </div>
      </Card>
      <EnableDialog open={dialog === 'enable'} onClose={() => setDialog(null)} />
      <PasswordDialog mode="disable" open={dialog === 'disable'} onClose={() => setDialog(null)} />
      <PasswordDialog mode="codes" open={dialog === 'codes'} onClose={() => setDialog(null)} />
    </Section>
  );
}

function useRefreshSession() {
  const queryClient = useQueryClient();
  return () => queryClient.fetchQuery({ ...sessionQuery, staleTime: 0 });
}

/** Password, then the code of the scanned QR code, then the backup codes. */
function EnableDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const t = useT();
  const refreshSession = useRefreshSession();
  const [step, setStep] = useState<{ kind: 'password' } | { kind: 'scan'; uri: string; codes: string[] } | { kind: 'codes'; codes: string[] }>({
    kind: 'password',
  });
  const [password, setPassword] = useState('');
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<MessageKey | null>(null);

  function close() {
    setStep({ kind: 'password' });
    setPassword('');
    setCode('');
    setError(null);
    onClose();
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (step.kind === 'password') {
        const result = await authClient.twoFactor.enable({ password });
        if (result.error || !result.data || !('totpURI' in result.data)) {
          setError(result.error ? failure(result.error) : 'errors.unknown');
          return;
        }
        setStep({ kind: 'scan', uri: result.data.totpURI, codes: result.data.backupCodes });
      } else if (step.kind === 'scan') {
        const result = await authClient.twoFactor.verifyTotp({ code: code.replace(/\s/g, '') });
        if (result.error) {
          setError(failure(result.error));
          return;
        }
        await refreshSession();
        setStep({ kind: 'codes', codes: step.codes });
      }
    } catch {
      setError('errors.network');
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(next) => !next && close()}>
      <DialogContent data-two-factor="">
        <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-5">
          <DialogHeader>
            <DialogTitle>{t('twoFactor.enableTitle')}</DialogTitle>
            <DialogDescription>
              {step.kind === 'password' ? t('twoFactor.passwordFirst') : step.kind === 'scan' ? t('twoFactor.scan') : t('twoFactor.codesHint')}
            </DialogDescription>
          </DialogHeader>
          {error ? (
            <p role="alert" className="text-sm text-destructive">
              {t(error)}
            </p>
          ) : null}
          {step.kind === 'password' ? (
            <Field label={t('account.currentPassword')}>
              {(control) => (
                <Input {...control} type="password" autoComplete="current-password" required value={password} onChange={(event) => setPassword(event.target.value)} />
              )}
            </Field>
          ) : step.kind === 'scan' ? (
            <>
              <QrCode value={step.uri} label={t('twoFactor.qrLabel')} className="size-48 self-center" />
              <p className="text-xs text-muted-foreground">
                {t('twoFactor.manual')} <code data-totp-secret="" className="break-all font-mono text-foreground">{secretOf(step.uri)}</code>
              </p>
              <Field label={t('twoFactor.code')}>
                {(control) => (
                  <Input
                    {...control}
                    required
                    inputMode="numeric"
                    autoComplete="one-time-code"
                    maxLength={7}
                    placeholder="123456"
                    className="font-mono tracking-widest"
                    value={code}
                    onChange={(event) => setCode(event.target.value)}
                  />
                )}
              </Field>
            </>
          ) : (
            <BackupCodes codes={step.codes} />
          )}
          <DialogFooter>
            {step.kind === 'codes' ? (
              <Button type="button" variant="primary" onClick={close}>
                {t('twoFactor.done')}
              </Button>
            ) : (
              <>
                <Button type="button" variant="ghost" onClick={close}>
                  {t('common.cancel')}
                </Button>
                <Button type="submit" variant="primary" loading={busy}>
                  {step.kind === 'password' ? t('twoFactor.next') : t('twoFactor.confirm')}
                </Button>
              </>
            )}
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** Switching it off, or new backup codes; both only with the password. */
function PasswordDialog({ mode, open, onClose }: { mode: 'disable' | 'codes'; open: boolean; onClose: () => void }) {
  const t = useT();
  const refreshSession = useRefreshSession();
  const [password, setPassword] = useState('');
  const [codes, setCodes] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<MessageKey | null>(null);

  function close() {
    setPassword('');
    setCodes(null);
    setError(null);
    onClose();
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (mode === 'disable') {
        const result = await authClient.twoFactor.disable({ password });
        if (result.error) {
          setError(failure(result.error));
          return;
        }
        await refreshSession();
        toast.success(t('twoFactor.disabled'));
        close();
      } else {
        const result = await authClient.twoFactor.generateBackupCodes({ password });
        if (result.error || !result.data) {
          setError(result.error ? failure(result.error) : 'errors.unknown');
          return;
        }
        setCodes(result.data.backupCodes);
      }
    } catch {
      setError('errors.network');
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={(next) => !next && close()}>
      <DialogContent>
        <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-5">
          <DialogHeader>
            <DialogTitle>{mode === 'disable' ? t('twoFactor.disableTitle') : t('twoFactor.newCodes')}</DialogTitle>
            <DialogDescription>
              {codes ? t('twoFactor.codesHint') : mode === 'disable' ? t('twoFactor.disableHint') : t('twoFactor.newCodesHint')}
            </DialogDescription>
          </DialogHeader>
          {error ? (
            <p role="alert" className="text-sm text-destructive">
              {t(error)}
            </p>
          ) : null}
          {codes ? (
            <BackupCodes codes={codes} />
          ) : (
            <Field label={t('account.currentPassword')}>
              {(control) => (
                <Input {...control} type="password" autoComplete="current-password" required value={password} onChange={(event) => setPassword(event.target.value)} />
              )}
            </Field>
          )}
          <DialogFooter>
            {codes ? (
              <Button type="button" variant="primary" onClick={close}>
                {t('twoFactor.done')}
              </Button>
            ) : (
              <>
                <Button type="button" variant="ghost" onClick={close}>
                  {t('common.cancel')}
                </Button>
                <Button type="submit" variant={mode === 'disable' ? 'destructive' : 'primary'} loading={busy}>
                  {mode === 'disable' ? t('twoFactor.disable') : t('twoFactor.newCodes')}
                </Button>
              </>
            )}
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

/** The backup codes, each good for one sign-in, with a button to copy them all. */
function BackupCodes({ codes }: { codes: string[] }) {
  const t = useT();
  return (
    <div className="flex flex-col gap-3">
      <ul data-backup-codes="" className="grid grid-cols-2 gap-2 rounded-lg bg-muted p-3 font-mono text-sm text-foreground">
        {codes.map((code) => (
          <li key={code}>{code}</li>
        ))}
      </ul>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="self-start"
        onClick={() => {
          void navigator.clipboard.writeText(codes.join('\n')).then(() => toast.success(t('common.copied')));
        }}
      >
        <Copy />
        {t('common.copy')}
      </Button>
    </div>
  );
}
