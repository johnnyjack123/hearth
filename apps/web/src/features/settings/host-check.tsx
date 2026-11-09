import type { HostCheckStatus } from '@harness/shared';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import { useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Card, Skeleton } from '@/components/ui/display.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { usePreferences } from '@/lib/preferences.tsx';
import { queryKeys, useModels, useProviders } from '@/lib/queries.ts';
import { withoutVendor } from '../chat/model-picker.tsx';
import { Section } from '../shell/page.tsx';

/**
 * Whether the server checks which OpenRouter hosts the user's account
 * allows, and what it found: for the models of the five latest chats, once
 * a week, or now with the button. Every allowed host costs a one-token
 * request, so it can be switched off.
 *
 * @returns The section, or nothing without an OpenRouter credential.
 */
export function HostCheckSection() {
  const providers = useProviders();
  return providers.data?.some((credential) => credential.kind === 'openrouter') ? <HostCheckCard /> : null;
}

function HostCheckCard() {
  const { t, locale } = usePreferences();
  const queryClient = useQueryClient();
  const status = useQuery({ queryKey: queryKeys.hostCheck, queryFn: api.openrouter.accessStatus });
  const models = useModels();
  const [busy, setBusy] = useState<'toggle' | 'refresh' | null>(null);

  // What the host lists show changes with every check.
  function settle(next: HostCheckStatus) {
    queryClient.setQueryData(queryKeys.hostCheck, next);
    void queryClient.invalidateQueries({ queryKey: ['openrouter-access'] });
  }

  async function run(kind: 'toggle' | 'refresh', action: () => Promise<HostCheckStatus>) {
    setBusy(kind);
    try {
      settle(await action());
      if (kind === 'refresh') {
        toast.success(t('hostCheck.done'));
      }
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setBusy(null);
    }
  }

  const name = (id: string) => {
    const model = models.data?.flatMap((group) => group.models).find((entry) => entry.id === id);
    return model ? withoutVendor(model.name) : id;
  };
  const date = (at: number) => new Date(at).toLocaleDateString(locale, { day: 'numeric', month: 'short' });
  const data = status.data;

  return (
    <Section title={t('hostCheck.title')} description={t('hostCheck.description')}>
      <Card className="flex flex-col gap-4 p-5">
        <label className="flex cursor-pointer items-start gap-3">
          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="text-sm font-medium text-foreground">{t('hostCheck.switch')}</span>
            <span className="text-xs text-muted-foreground">{t('hostCheck.cost')}</span>
          </span>
          <Switch
            checked={data?.enabled ?? false}
            disabled={!data || busy !== null}
            onCheckedChange={(enabled) => void run('toggle', () => api.openrouter.setAccessCheck(enabled))}
            aria-label={t('hostCheck.switch')}
          />
        </label>
        {!data ? (
          <Skeleton className="h-10 w-full" />
        ) : data.enabled ? (
          <>
            {data.models.length === 0 ? (
              <p className="text-sm text-muted-foreground">{t('hostCheck.none')}</p>
            ) : (
              <ul className="flex flex-col gap-2">
                {data.models.map((entry) => (
                  <li key={`${entry.credentialId} ${entry.model}`} className="flex flex-col gap-0.5 text-sm">
                    <span className="text-foreground">{name(entry.model)}</span>
                    <span className="text-xs text-muted-foreground">
                      {entry.checkedAt === null
                        ? t('hostCheck.never')
                        : t('hostCheck.result', { date: date(entry.checkedAt), allowed: String(entry.allowed), blocked: String(entry.blocked) })}
                    </span>
                  </li>
                ))}
              </ul>
            )}
            <Button
              variant="secondary"
              className="self-start"
              loading={busy === 'refresh'}
              disabled={busy !== null || data.models.length === 0}
              onClick={() => void run('refresh', api.openrouter.refreshAccess)}
            >
              <RefreshCw />
              {t('hostCheck.refresh')}
            </Button>
          </>
        ) : (
          <p className="text-sm text-muted-foreground">{t('hostCheck.off')}</p>
        )}
      </Card>
    </Section>
  );
}
