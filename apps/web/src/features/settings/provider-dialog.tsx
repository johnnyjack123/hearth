import { PROVIDER_KINDS, PROVIDERS, type ProviderCredential, type ProviderKind } from '@hearth/shared';
import { useQueryClient } from '@tanstack/react-query';
import { type FormEvent, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { useT } from '@/lib/preferences.tsx';
import { providerName } from '@/lib/providers.ts';
import { queryKeys } from '@/lib/queries.ts';
import { cn } from '@/lib/utils.ts';
import { testCredential } from './test-credential.ts';

/**
 * Props of {@link ProviderDialog}.
 */
export interface ProviderDialogProps {
  /** Whether the dialog is open. */
  open: boolean;
  /** Called when the dialog wants to close. */
  onOpenChange: (open: boolean) => void;
  /** Credential to edit; omitted to create a new one. */
  credential?: ProviderCredential | undefined;
  /** Whether the user may point credentials at custom endpoints. */
  customAllowed: boolean;
}

/**
 * Create or edit a provider credential. New credentials are tested right
 * after saving so a typo in the key shows up immediately.
 *
 * @param props - See {@link ProviderDialogProps}.
 * @returns The dialog.
 */
export function ProviderDialog({ open, onOpenChange, credential, customAllowed }: ProviderDialogProps) {
  const t = useT();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{credential ? t('providers.editTitle') : t('providers.addTitle')}</DialogTitle>
          <DialogDescription>{t('providers.description')}</DialogDescription>
        </DialogHeader>
        {/* Remount per opening so the form starts from the current credential. */}
        {open ? (
          <ProviderForm credential={credential} customAllowed={customAllowed} onDone={() => onOpenChange(false)} />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

function ProviderForm({
  credential,
  customAllowed,
  onDone,
}: {
  credential: ProviderCredential | undefined;
  customAllowed: boolean;
  onDone: () => void;
}) {
  const t = useT();
  const queryClient = useQueryClient();
  const [kind, setKind] = useState<ProviderKind>(credential?.kind ?? 'anthropic');
  const [label, setLabel] = useState(credential?.label ?? providerName(t, 'anthropic'));
  const [labelTouched, setLabelTouched] = useState(Boolean(credential));
  const [apiKey, setApiKey] = useState('');
  const [baseUrl, setBaseUrl] = useState(credential?.baseUrl ?? '');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const info = PROVIDERS[kind];
  const kindLocked = (candidate: ProviderKind) => PROVIDERS[candidate].userDefinedEndpoint && !customAllowed;
  const baseUrlEditable = info.baseUrl !== 'fixed' && (customAllowed || info.baseUrl === 'required');

  function chooseKind(next: ProviderKind) {
    setKind(next);
    setBaseUrl('');
    if (!labelTouched) {
      setLabel(providerName(t, next));
    }
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    setSaving(true);
    setError(null);
    const trimmedUrl = baseUrl.trim();
    try {
      let saved: ProviderCredential;
      if (credential) {
        saved = await api.providers.update(credential.id, {
          ...(label.trim() !== credential.label ? { label: label.trim() } : {}),
          ...(apiKey.trim() ? { apiKey: apiKey.trim() } : {}),
          ...(baseUrlEditable && trimmedUrl !== (credential.baseUrl ?? '') ? { baseUrl: trimmedUrl || null } : {}),
        });
        toast.success(t('providers.updated'));
      } else {
        saved = await api.providers.create({
          kind,
          label: label.trim(),
          ...(apiKey.trim() ? { apiKey: apiKey.trim() } : {}),
          ...(trimmedUrl ? { baseUrl: trimmedUrl } : {}),
        });
        toast.success(t('providers.created'));
      }
      await queryClient.invalidateQueries({ queryKey: queryKeys.providers });
      await queryClient.invalidateQueries({ queryKey: queryKeys.models });
      onDone();
      if (!credential || apiKey.trim()) {
        void testCredential(t, saved);
      }
    } catch (caught) {
      setError(errorMessage(t, caught));
    } finally {
      setSaving(false);
    }
  }

  return (
    <form onSubmit={submit} className="flex flex-col gap-5">
      {credential ? null : (
        <fieldset className="flex flex-col gap-2">
          <legend className="mb-2 text-sm font-medium text-foreground">{t('providers.kind')}</legend>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
            {PROVIDER_KINDS.map((candidate) => {
              const locked = kindLocked(candidate);
              return (
                <label
                  key={candidate}
                  className={cn(
                    'flex cursor-pointer items-center justify-center rounded-lg border border-border bg-card px-3 py-2.5 text-center text-sm font-medium text-foreground',
                    'transition-[border-color,background-color,box-shadow] duration-(--duration-fast) ease-standard hover:bg-accent',
                    'has-checked:border-primary has-checked:bg-primary/8 has-checked:shadow-elevation-1 has-focus-visible:ring-(length:--focus-ring-width) has-focus-visible:ring-ring',
                    locked && 'cursor-not-allowed opacity-50 hover:bg-card',
                  )}
                >
                  <input
                    type="radio"
                    name="provider-kind"
                    value={candidate}
                    checked={kind === candidate}
                    disabled={locked}
                    onChange={() => chooseKind(candidate)}
                    className="sr-only"
                  />
                  {providerName(t, candidate)}
                </label>
              );
            })}
          </div>
          {!customAllowed ? <p className="text-xs text-muted-foreground">{t('providers.customEndpointsDisabled')}</p> : null}
        </fieldset>
      )}

      <Field label={t('providers.label')}>
        {(control) => (
          <Input
            {...control}
            required
            maxLength={60}
            value={label}
            placeholder={t('providers.labelPlaceholder')}
            onChange={(event) => {
              setLabel(event.target.value);
              setLabelTouched(true);
            }}
          />
        )}
      </Field>

      {info.apiKey === 'none' ? null : (
        <Field
          label={t('providers.apiKey')}
          labelSuffix={info.apiKey === 'optional' ? `(${t('common.optional')})` : undefined}
          hint={credential?.keyHint ? t('providers.apiKeyKeep') : undefined}
        >
          {(control) => (
            <Input
              {...control}
              type="password"
              autoComplete="off"
              spellCheck={false}
              required={info.apiKey === 'required' && !credential}
              value={apiKey}
              placeholder={credential?.keyHint ?? ''}
              onChange={(event) => setApiKey(event.target.value)}
              className="font-mono"
            />
          )}
        </Field>
      )}

      {info.baseUrl === 'fixed' ? null : (
        <Field
          label={t('providers.baseUrl')}
          labelSuffix={info.baseUrl === 'optional' ? `(${t('common.optional')})` : undefined}
          hint={info.baseUrl === 'required' ? t('providers.baseUrlRequired') : customAllowed ? t('providers.baseUrlHint') : t('providers.customEndpointsDisabled')}
        >
          {(control) => (
            <Input
              {...control}
              type="url"
              inputMode="url"
              spellCheck={false}
              required={info.baseUrl === 'required'}
              disabled={!baseUrlEditable}
              value={baseUrl}
              placeholder={info.defaultBaseUrl ?? 'http://localhost:8000/v1'}
              onChange={(event) => setBaseUrl(event.target.value)}
              className="font-mono"
            />
          )}
        </Field>
      )}

      {error ? (
        <p role="alert" className="rounded-lg bg-destructive-subtle p-3 text-sm text-destructive">
          {error}
        </p>
      ) : null}

      <DialogFooter>
        <Button type="button" variant="ghost" onClick={onDone}>
          {t('common.cancel')}
        </Button>
        <Button type="submit" variant="primary" loading={saving}>
          {t('common.save')}
        </Button>
      </DialogFooter>
    </form>
  );
}
