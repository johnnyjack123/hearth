import type { CostStatus } from '@harness/shared';
import { useQueryClient } from '@tanstack/react-query';
import { type FormEvent, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Card, Skeleton } from '@/components/ui/display.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { usePreferences } from '@/lib/preferences.tsx';
import { queryKeys, useCosts } from '@/lib/queries.ts';
import { Section } from '../shell/page.tsx';
import { formatDollars } from '../chat/usage-details.tsx';

/**
 * Reads a dollar amount as typed, with a comma or a dot.
 *
 * @param text - The input.
 * @returns The amount, null for an empty input, undefined if it is no positive amount.
 */
export function parseDollars(text: string): number | null | undefined {
  const trimmed = text.trim().replace(',', '.');
  if (!trimmed) {
    return null;
  }
  const value = Number(trimmed);
  return Number.isFinite(value) && value > 0 && value <= 100_000 ? value : undefined;
}

/**
 * A limit as the input shows it, in the UI language (`2,50` in German).
 *
 * @param locale - UI language.
 * @param dollars - The limit, or null.
 * @returns The text, empty for none.
 */
export function dollarsInput(locale: string, dollars: number | null): string {
  return dollars === null ? '' : new Intl.NumberFormat(locale, { minimumFractionDigits: 2, maximumFractionDigits: 6, useGrouping: false }).format(dollars);
}

/**
 * What the user spent today and their own daily limit, in the account
 * settings. Chats and agents stop at the limit until the next day.
 *
 * @returns The section.
 */
export function CostLimitSection() {
  const { t, locale } = usePreferences();
  const queryClient = useQueryClient();
  const costs = useCosts();
  const [limit, setLimit] = useState('');
  const [saving, setSaving] = useState(false);
  const own = costs.data?.own ?? null;

  useEffect(() => {
    setLimit(dollarsInput(locale, own));
  }, [own, locale]);

  async function save(event: FormEvent) {
    event.preventDefault();
    const daily = parseDollars(limit);
    if (daily === undefined) {
      toast.error(t('costs.invalid'));
      return;
    }
    setSaving(true);
    try {
      queryClient.setQueryData<CostStatus>(queryKeys.costs, await api.costs.setLimit(daily));
      toast.success(t('costs.saved'));
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setSaving(false);
    }
  }

  const status = costs.data;
  return (
    <Section title={t('costs.title')} description={t('costs.description')}>
      <Card className="flex flex-col gap-4 p-5">
        {status ? (
          <div className="flex flex-col gap-1 text-sm">
            <span className="text-foreground">{t('costs.spent', { day: status.day, spent: formatDollars(locale, status.spent) })}</span>
            <span className="text-muted-foreground">
              {status.limit === null
                ? t('costs.noLimit')
                : t('costs.limit', { limit: formatDollars(locale, status.limit), left: formatDollars(locale, Math.max(0, status.limit - status.spent)) })}
            </span>
            {status.server !== null ? (
              <span className="text-xs text-muted-foreground">{t('costs.serverLimit', { limit: formatDollars(locale, status.server) })}</span>
            ) : null}
          </div>
        ) : (
          <Skeleton className="h-10 w-full" />
        )}
        <form onSubmit={(event) => void save(event)} className="flex flex-col gap-3 sm:flex-row sm:items-end">
          <Field label={t('costs.own')} hint={t('costs.ownHint')} className="flex-1">
            {(control) => <Input {...control} inputMode="decimal" placeholder="1,00" value={limit} onChange={(event) => setLimit(event.target.value)} />}
          </Field>
          <Button type="submit" variant="secondary" loading={saving} disabled={limit.trim() === dollarsInput(locale, own)}>
            {t('costs.save')}
          </Button>
        </form>
      </Card>
    </Section>
  );
}
