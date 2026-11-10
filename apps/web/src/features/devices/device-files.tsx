import { type AppFileResult, appNameSchema, type DeviceSummary } from '@hearth/shared';
import { useQueryClient } from '@tanstack/react-query';
import { Check, Copy, FileUp, Hourglass } from 'lucide-react';
import { type FormEvent, useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { usePreferences } from '@/lib/preferences.tsx';
import { queryKeys } from '@/lib/queries.ts';
import { shortName } from './own-apps.tsx';

/**
 * The app name a file suggests, without extension and version, like
 * `suggested_name` in hearth-core: `rg`, `tool` for `tool-1.2.tar.gz`,
 * `fd` for `fd_10.2.0_arm64.deb`.
 *
 * @param path - A Windows or Linux path.
 * @returns A short name, `programm` if nothing is left.
 */
export function nameFromPath(path: string): string {
  let stem = (path.split(/[\\/]/).pop() ?? '').toLowerCase();
  for (const suffix of ['.tar.gz', '.tgz', '.tar.xz', '.txz', '.zip', '.deb', '.appimage']) {
    if (stem.endsWith(suffix)) {
      stem = stem.slice(0, -suffix.length);
      break;
    }
  }
  stem = stem.split('_')[0] ?? stem;
  const parts = stem.split(/[-.]/);
  const version = parts.findIndex((part) => /^v?\d/.test(part));
  const name = shortName((version > 0 ? parts.slice(0, version) : parts).join('-'));
  return name || 'programm';
}

/**
 * Program files of one computer for Settings → Devices: the requests still
 * waiting for approval there, and asking for a new one. The browser only
 * asks; the file goes into the VM once the user approves it on the
 * computer (tray icon or `hearth app approve`).
 *
 * @param props - The device.
 * @returns The section.
 */
export function DeviceFiles({ device }: { device: DeviceSummary }) {
  const { t } = usePreferences();
  const [open, setOpen] = useState(false);
  const pending = device.apps?.pending ?? [];
  return (
    <div className="flex min-w-0 flex-col gap-1 pt-1">
      {pending.map((file) => (
        <div key={file.id} className="flex min-w-0 items-start gap-2 rounded-lg bg-muted px-3 py-2 text-xs">
          <Hourglass className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span className="flex min-w-0 flex-col gap-0.5">
            <span className="text-foreground">{t('devices.files.pending', { title: file.title })}</span>
            <span className="break-all text-muted-foreground">
              {file.kind} · {file.path}
            </span>
            <span className="text-muted-foreground">
              {t('devices.files.pendingHow')} <code className="font-mono text-foreground">hearth app approve {file.id}</code>
            </span>
          </span>
        </div>
      ))}
      <Button
        variant="ghost"
        size="sm"
        className="-ms-2 self-start px-2 text-muted-foreground hover:text-foreground"
        disabled={!device.online}
        title={device.online ? undefined : t('devices.files.offline')}
        onClick={() => setOpen(true)}
      >
        <FileUp />
        {t('devices.files.add')}
      </Button>
      <AppFileDialog open={open} onOpenChange={setOpen} device={device} />
    </div>
  );
}

function AppFileDialog({ open, onOpenChange, device }: { open: boolean; onOpenChange: (open: boolean) => void; device: DeviceSummary }) {
  const { t } = usePreferences();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('devices.files.dialogTitle', { device: device.name })}</DialogTitle>
          <DialogDescription>{t('devices.files.dialogDescription')}</DialogDescription>
        </DialogHeader>
        {/* Remount per opening so the form starts empty. */}
        {open ? <AppFileForm device={device} onDone={() => onOpenChange(false)} /> : null}
      </DialogContent>
    </Dialog>
  );
}

function AppFileForm({ device, onDone }: { device: DeviceSummary; onDone: () => void }) {
  const { t } = usePreferences();
  const queryClient = useQueryClient();
  const [path, setPath] = useState('');
  const [title, setTitle] = useState('');
  const [titleTouched, setTitleTouched] = useState(false);
  const [name, setName] = useState('');
  const [nameTouched, setNameTouched] = useState(false);
  const [checked, setChecked] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<AppFileResult | null>(null);

  const nameError = appNameSchema.safeParse(name).success ? undefined : t('ownApps.nameInvalid');

  async function submit(event: FormEvent) {
    event.preventDefault();
    setChecked(true);
    if (nameError || !path.trim() || !title.trim()) {
      return;
    }
    setSending(true);
    setError(null);
    try {
      const answer = await api.devices.requestAppFile(device.id, { path: path.trim(), name, title: title.trim() });
      if (answer.ok) {
        setResult(answer);
        void queryClient.invalidateQueries({ queryKey: queryKeys.devices });
      } else {
        setError(answer.message);
      }
    } catch (caught) {
      setError(errorMessage(t, caught));
    } finally {
      setSending(false);
    }
  }

  if (result?.approval) {
    return <Waiting approval={result.approval} title={title.trim()} onDone={onDone} />;
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-5" noValidate>
      <Field label={t('devices.files.path')} hint={t('devices.files.pathHint')}>
        {(control) => (
          <Input
            {...control}
            required
            spellCheck={false}
            autoCapitalize="off"
            maxLength={1000}
            value={path}
            placeholder={'C:\\Tools\\programm.AppImage'}
            onChange={(event) => {
              const next = event.target.value;
              setPath(next);
              const suggested = nameFromPath(next.trim().replace(/^"|"$/g, ''));
              if (!titleTouched) {
                setTitle(suggested);
              }
              if (!nameTouched) {
                setName(suggested);
              }
            }}
            className="font-mono"
          />
        )}
      </Field>
      <div className="grid gap-5 sm:grid-cols-2">
        <Field label={t('ownApps.titleLabel')}>
          {(control) => (
            <Input
              {...control}
              required
              maxLength={60}
              value={title}
              onChange={(event) => {
                setTitle(event.target.value);
                setTitleTouched(true);
              }}
            />
          )}
        </Field>
        <Field label={t('ownApps.name')} hint={t('ownApps.nameHint')} error={checked ? nameError : undefined}>
          {(control) => (
            <Input
              {...control}
              required
              maxLength={30}
              spellCheck={false}
              value={name}
              onChange={(event) => {
                setName(event.target.value);
                setNameTouched(true);
              }}
              className="font-mono"
            />
          )}
        </Field>
      </div>
      {error ? (
        <p role="alert" className="rounded-lg bg-destructive-subtle p-3 text-sm text-destructive">
          {error}
        </p>
      ) : null}
      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onDone}>
          {t('common.cancel')}
        </Button>
        <Button type="submit" variant="primary" loading={sending}>
          {t('devices.files.submit')}
        </Button>
      </DialogFooter>
    </form>
  );
}

/** What to do next, once the computer holds the request. */
function Waiting({ approval, title, onDone }: { approval: string; title: string; onDone: () => void }) {
  const { t } = usePreferences();
  const [copied, setCopied] = useState(false);
  const command = `hearth app approve ${approval}`;
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-col gap-2 rounded-lg bg-muted p-4">
        <span className="text-sm font-medium text-foreground">{t('devices.files.waitingTitle')}</span>
        <span className="text-sm text-muted-foreground">{t('devices.files.waitingTray', { title })}</span>
        <span className="text-sm text-muted-foreground">{t('devices.files.waitingTerminal')}</span>
        <div className="flex min-w-0 items-center gap-2">
          <code className="min-w-0 flex-1 truncate rounded-md bg-card px-2 py-1.5 font-mono text-sm text-foreground">{command}</code>
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              void navigator.clipboard.writeText(command).then(() => setCopied(true));
            }}
          >
            {copied ? <Check /> : <Copy />}
            {copied ? t('devices.files.copied') : t('devices.files.copy')}
          </Button>
        </div>
      </div>
      <DialogFooter>
        <Button variant="primary" onClick={onDone}>
          {t('common.close')}
        </Button>
      </DialogFooter>
    </div>
  );
}
