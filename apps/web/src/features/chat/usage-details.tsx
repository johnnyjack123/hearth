import type { MessageMetadata, ModelCallUsage } from '@harness/shared';
import type { ReactNode } from 'react';
import { usePreferences } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

type Usage = NonNullable<MessageMetadata['usage']>;

/**
 * Formats dollars for the usage details: tiny amounts keep three
 * significant digits, so a call for a hundredth of a cent does not read as 0.
 *
 * @param locale - UI language.
 * @param dollars - Amount.
 * @returns The formatted amount.
 */
export function formatDollars(locale: string, dollars: number): string {
  if (dollars === 0) {
    return new Intl.NumberFormat(locale, { style: 'currency', currency: 'USD', maximumFractionDigits: 0 }).format(0);
  }
  return new Intl.NumberFormat(locale, {
    style: 'currency',
    currency: 'USD',
    ...(Math.abs(dollars) < 0.01 ? { maximumSignificantDigits: 3 } : { minimumFractionDigits: 2, maximumFractionDigits: 4 }),
  }).format(dollars);
}

function sum(calls: ModelCallUsage[], pick: (call: ModelCallUsage) => number | undefined): number {
  return calls.reduce((total, call) => total + (pick(call) ?? 0), 0);
}

function Cell({ children, head, start }: { children: ReactNode; head?: boolean; start?: boolean }) {
  const Tag = head ? 'th' : 'td';
  return (
    <Tag
      scope={head ? 'col' : undefined}
      className={cn('whitespace-nowrap px-2 py-1.5', start ? 'text-start' : 'text-end', head && 'font-medium text-muted-foreground')}
    >
      {children}
    </Tag>
  );
}

type Purpose = NonNullable<ModelCallUsage['purpose']> | 'answer';

/**
 * The reply's own calls and its helper calls (Jev, embeddings, title),
 * summed per model and purpose.
 *
 * @param usage - The reply's usage.
 * @param modelId - The chat's model.
 * @returns One row per model and purpose, the reply's own calls first.
 */
export function costByModel(usage: Usage, modelId: string | undefined) {
  const rows = new Map<string, { model: string; purpose: Purpose; calls: number; input: number; output: number; cost: number | undefined }>();
  const add = (call: ModelCallUsage, model: string, purpose: Purpose) => {
    const key = `${model}|${purpose}`;
    const row = rows.get(key) ?? { model, purpose, calls: 0, input: 0, output: 0, cost: 0 };
    row.calls += 1;
    row.input += call.input;
    row.output += call.output;
    row.cost = row.cost === undefined || call.cost === undefined ? undefined : row.cost + call.cost.total;
    rows.set(key, row);
  };
  for (const call of usage.calls ?? []) {
    add(call, modelId ?? '–', 'answer');
  }
  for (const call of usage.helpers ?? []) {
    add(call, call.model ?? '–', call.purpose ?? 'answer');
  }
  return [...rows.values()];
}

/**
 * Tokens and cost of every model call of a reply, behind a button below it:
 * how much each call sent anew, how much came from the provider's cache,
 * how much it wrote and reasoned, and what each part cost; with helper
 * models, also the cost per model.
 *
 * @param props - The reply's usage, its model and whether an agent produced it.
 * @returns The tables.
 */
export function UsageDetails({ usage, agent, modelId }: { usage: Usage; agent: boolean; modelId?: string }) {
  const { t, locale } = usePreferences();
  const tokens = new Intl.NumberFormat(locale);
  const calls = usage.calls ?? [];
  const byModel = costByModel(usage, modelId);
  const input = sum(calls, (call) => call.input);
  const cached = sum(calls, (call) => call.cacheRead);
  const withCacheWrite = calls.some((call) => call.cacheWrite > 0);
  const withSplitCost = calls.some((call) => call.cost?.input !== undefined);
  const dollars = (value: number | undefined) => (value === undefined ? t('chat.usageDetails.unknown') : formatDollars(locale, value));
  const result = (call: ModelCallUsage) => (call.tools.length > 0 ? call.tools.join(', ') : agent ? '–' : t('chat.usageDetails.answer'));
  const table = 'w-full border-collapse text-xs tabular-nums [&_tbody_tr]:border-t [&_tbody_tr]:border-border [&_tfoot_tr]:border-t-2 [&_tfoot_tr]:border-border';

  if (calls.length === 0) {
    return <p className="text-xs text-muted-foreground">{t('chat.usageDetails.noCalls')}</p>;
  }
  return (
    <div className="flex flex-col gap-3 text-xs text-foreground">
      <div className="overflow-x-auto rounded-lg border border-border bg-card">
        <table className={table}>
          <caption className="px-2 pt-2 text-start font-medium text-muted-foreground">{t('chat.usageDetails.tokens')}</caption>
          <thead>
            <tr>
              <Cell head start>
                {t('chat.usageDetails.call')}
              </Cell>
              <Cell head start>
                {t('chat.usageDetails.result')}
              </Cell>
              <Cell head>{t('chat.usageDetails.input')}</Cell>
              <Cell head>{t('chat.usageDetails.cached')}</Cell>
              <Cell head>{t('chat.usageDetails.fresh')}</Cell>
              {withCacheWrite ? <Cell head>{t('chat.usageDetails.cacheWrite')}</Cell> : null}
              <Cell head>{t('chat.usageDetails.output')}</Cell>
              <Cell head>{t('chat.usageDetails.reasoning')}</Cell>
            </tr>
          </thead>
          <tbody>
            {calls.map((call, index) => (
              <tr key={index}>
                <Cell start>{index + 1}</Cell>
                <Cell start>{result(call)}</Cell>
                <Cell>{tokens.format(call.input)}</Cell>
                <Cell>{tokens.format(call.cacheRead)}</Cell>
                <Cell>{tokens.format(Math.max(0, call.input - call.cacheRead - call.cacheWrite))}</Cell>
                {withCacheWrite ? <Cell>{tokens.format(call.cacheWrite)}</Cell> : null}
                <Cell>{tokens.format(call.output)}</Cell>
                <Cell>{call.reasoning === undefined ? '–' : tokens.format(call.reasoning)}</Cell>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="font-medium">
              <Cell start>{t('chat.usageDetails.sum')}</Cell>
              <Cell start>{''}</Cell>
              <Cell>{tokens.format(input)}</Cell>
              <Cell>{tokens.format(cached)}</Cell>
              <Cell>{tokens.format(sum(calls, (call) => Math.max(0, call.input - call.cacheRead - call.cacheWrite)))}</Cell>
              {withCacheWrite ? <Cell>{tokens.format(sum(calls, (call) => call.cacheWrite))}</Cell> : null}
              <Cell>{tokens.format(sum(calls, (call) => call.output))}</Cell>
              <Cell>{tokens.format(sum(calls, (call) => call.reasoning))}</Cell>
            </tr>
          </tfoot>
        </table>
      </div>

      <div className="overflow-x-auto rounded-lg border border-border bg-card">
        <table className={table}>
          <caption className="px-2 pt-2 text-start font-medium text-muted-foreground">{t('chat.usageDetails.costs')}</caption>
          <thead>
            <tr>
              <Cell head start>
                {t('chat.usageDetails.call')}
              </Cell>
              {withSplitCost ? (
                <>
                  <Cell head>{t('chat.usageDetails.fresh')}</Cell>
                  <Cell head>{t('chat.usageDetails.cached')}</Cell>
                  {withCacheWrite ? <Cell head>{t('chat.usageDetails.cacheWrite')}</Cell> : null}
                  <Cell head>{t('chat.usageDetails.output')}</Cell>
                </>
              ) : null}
              <Cell head>{t('chat.usageDetails.total')}</Cell>
            </tr>
          </thead>
          <tbody>
            {calls.map((call, index) => (
              <tr key={index}>
                <Cell start>{index + 1}</Cell>
                {withSplitCost ? (
                  <>
                    <Cell>{dollars(call.cost?.input)}</Cell>
                    <Cell>{dollars(call.cost?.cacheRead)}</Cell>
                    {withCacheWrite ? <Cell>{dollars(call.cost?.cacheWrite)}</Cell> : null}
                    <Cell>{dollars(call.cost?.output)}</Cell>
                  </>
                ) : null}
                <Cell>{dollars(call.cost?.total)}</Cell>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="font-medium">
              <Cell start>{t('chat.usageDetails.sum')}</Cell>
              {withSplitCost ? (
                <>
                  <Cell>{dollars(sum(calls, (call) => call.cost?.input))}</Cell>
                  <Cell>{dollars(sum(calls, (call) => call.cost?.cacheRead))}</Cell>
                  {withCacheWrite ? <Cell>{dollars(sum(calls, (call) => call.cost?.cacheWrite))}</Cell> : null}
                  <Cell>{dollars(sum(calls, (call) => call.cost?.output))}</Cell>
                </>
              ) : null}
              <Cell>{dollars(usage.cost)}</Cell>
            </tr>
          </tfoot>
        </table>
      </div>

      {byModel.length > 1 ? (
        <div className="overflow-x-auto rounded-lg border border-border bg-card">
          <table className={table}>
            <caption className="px-2 pt-2 text-start font-medium text-muted-foreground">{t('chat.usageDetails.byModel')}</caption>
            <thead>
              <tr>
                <Cell head start>
                  {t('chat.usageDetails.model')}
                </Cell>
                <Cell head start>
                  {t('chat.usageDetails.purpose')}
                </Cell>
                <Cell head>{t('chat.usageDetails.calls')}</Cell>
                <Cell head>{t('chat.usageDetails.input')}</Cell>
                <Cell head>{t('chat.usageDetails.output')}</Cell>
                <Cell head>{t('chat.usageDetails.total')}</Cell>
              </tr>
            </thead>
            <tbody>
              {byModel.map((row) => (
                <tr key={`${row.model}|${row.purpose}`}>
                  <Cell start>{row.model}</Cell>
                  <Cell start>{t(`chat.usageDetails.purposes.${row.purpose}`)}</Cell>
                  <Cell>{tokens.format(row.calls)}</Cell>
                  <Cell>{tokens.format(row.input)}</Cell>
                  <Cell>{tokens.format(row.output)}</Cell>
                  <Cell>{dollars(row.cost)}</Cell>
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr className="font-medium">
                <Cell start>{t('chat.usageDetails.sum')}</Cell>
                <Cell start>{''}</Cell>
                <Cell>{tokens.format(byModel.reduce((total, row) => total + row.calls, 0))}</Cell>
                <Cell>{tokens.format(byModel.reduce((total, row) => total + row.input, 0))}</Cell>
                <Cell>{tokens.format(byModel.reduce((total, row) => total + row.output, 0))}</Cell>
                <Cell>{dollars(usage.cost)}</Cell>
              </tr>
            </tfoot>
          </table>
        </div>
      ) : null}

      <div className="flex flex-col gap-1 text-muted-foreground">
        {input > 0 ? <p>{t('chat.usageDetails.cacheShare', { percent: Math.round((cached / input) * 100) })}</p> : null}
        {usage.rates ? (
          <p>
            {t('chat.usageDetails.rates', {
              input: formatDollars(locale, usage.rates.input),
              cacheRead: formatDollars(locale, usage.rates.cacheRead ?? usage.rates.input),
              output: formatDollars(locale, usage.rates.output),
            })}
          </p>
        ) : null}
        <p>{agent ? t('chat.usageDetails.agent') : t('chat.usageDetails.repeat')}</p>
      </div>
    </div>
  );
}
