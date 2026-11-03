import type { CompactSettings, CondenseSettings, SearchProvider, SearchSettings, SearchTestResult, ServerSettings, UpdateServerSettings } from '@harness/shared';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { CircleAlert, CircleCheck } from 'lucide-react';
import { type FormEvent, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Card, Separator, Skeleton } from '@/components/ui/display.tsx';
import { Field } from '@/components/ui/field.tsx';
import { Input } from '@/components/ui/input.tsx';
import { Switch } from '@/components/ui/switch.tsx';
import { SegmentedControl } from '@/components/ui/tabs.tsx';
import type { MessageKey, TranslateFn } from '@/i18n/index.ts';
import { api, errorMessage } from '@/lib/api.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys, usePublicSettings } from '@/lib/queries.ts';
import { cn } from '@/lib/utils.ts';
import { Section, SettingRow } from '../shell/page.tsx';

/**
 * Server-wide settings: registration mode, custom endpoints, web fetch and
 * the web search behind research mode.
 *
 * @returns The server panel.
 */
export function ServerPanel() {
  const t = useT();
  const { data: publicSettings } = usePublicSettings();
  const queryClient = useQueryClient();
  const settings = useQuery({ queryKey: queryKeys.serverSettings, queryFn: api.settings.get });
  const update = useMutation({
    mutationFn: (patch: Omit<UpdateServerSettings, 'search' | 'condense' | 'compact'>) => api.settings.update(patch),
    onMutate: async (patch) => {
      // Optimistic: switches react instantly and roll back on failure.
      await queryClient.cancelQueries({ queryKey: queryKeys.serverSettings });
      const previous = queryClient.getQueryData<ServerSettings>(queryKeys.serverSettings);
      queryClient.setQueryData<ServerSettings>(queryKeys.serverSettings, (current) => (current ? { ...current, ...patch } : current));
      return { previous };
    },
    onError: (error, _patch, context) => {
      queryClient.setQueryData(queryKeys.serverSettings, context?.previous);
      toast.error(errorMessage(t, error));
    },
    onSuccess: (next) => {
      queryClient.setQueryData(queryKeys.serverSettings, next);
      void queryClient.invalidateQueries({ queryKey: queryKeys.publicSettings });
      toast.success(t('admin.saved'));
    },
  });

  return (
    <div className="flex flex-col gap-10">
      <Section
        title={t('admin.tabServer')}
        description={`${t('admin.serverDescription')} ${t('admin.serverVersion', {
          version: publicSettings?.version.server ?? '…',
          commit: publicSettings?.version.commit ?? '–',
        })}`}
      >
        <Card>
          {settings.data ? (
            <>
              <SettingRow
                label={t('admin.registration')}
                description={t('admin.registrationHint')}
                control={
                  <Switch
                    aria-label={t('admin.registration')}
                    checked={settings.data.registration === 'open'}
                    onCheckedChange={(checked) => update.mutate({ registration: checked ? 'open' : 'closed' })}
                  />
                }
              />
              <Separator />
              <SettingRow
                label={t('admin.customBaseUrls')}
                description={t('admin.customBaseUrlsHint')}
                control={
                  <Switch
                    aria-label={t('admin.customBaseUrls')}
                    checked={settings.data.allowCustomBaseUrls}
                    onCheckedChange={(checked) => update.mutate({ allowCustomBaseUrls: checked })}
                  />
                }
              />
              <Separator />
              <SettingRow
                label={t('admin.webFetch')}
                description={t('admin.webFetchHint')}
                control={
                  <Switch
                    aria-label={t('admin.webFetch')}
                    checked={settings.data.webFetch}
                    onCheckedChange={(checked) => update.mutate({ webFetch: checked })}
                  />
                }
              />
              <Separator />
              <SettingRow
                label={t('admin.calculator')}
                description={t('admin.calculatorHint')}
                control={
                  <Switch
                    aria-label={t('admin.calculator')}
                    checked={settings.data.calculator}
                    onCheckedChange={(checked) => update.mutate({ calculator: checked })}
                  />
                }
              />
              <Separator />
              <SettingRow
                label={t('admin.memory')}
                description={t('admin.memoryHint')}
                control={
                  <Switch
                    aria-label={t('admin.memory')}
                    checked={settings.data.memory}
                    onCheckedChange={(checked) => update.mutate({ memory: checked })}
                  />
                }
              />
              <Separator />
              <SettingRow
                label={t('admin.progress')}
                description={t('admin.progressHint')}
                control={
                  <Switch
                    aria-label={t('admin.progress')}
                    checked={settings.data.progress}
                    onCheckedChange={(checked) => update.mutate({ progress: checked })}
                  />
                }
              />
              <Separator />
              <SettingRow
                label={t('admin.openRouterCheapest')}
                description={t('admin.openRouterCheapestHint')}
                control={
                  <Switch
                    aria-label={t('admin.openRouterCheapest')}
                    checked={settings.data.openRouterCheapest}
                    onCheckedChange={(checked) => update.mutate({ openRouterCheapest: checked })}
                  />
                }
              />
              <Separator />
              <SettingRow
                label={t('admin.openRouterNoFourBit')}
                description={t('admin.openRouterNoFourBitHint')}
                control={
                  <Switch
                    aria-label={t('admin.openRouterNoFourBit')}
                    checked={settings.data.openRouterNoFourBit}
                    disabled={!settings.data.openRouterCheapest}
                    onCheckedChange={(checked) => update.mutate({ openRouterNoFourBit: checked })}
                  />
                }
              />
            </>
          ) : (
            <div className="flex flex-col gap-4 p-4">
              <Skeleton className="h-12 w-full" />
              <Skeleton className="h-12 w-full" />
              <Skeleton className="h-12 w-full" />
            </div>
          )}
        </Card>
      </Section>
      <Section title={t('admin.search.title')} description={t('admin.search.description')}>
        {settings.data ? <SearchSettingsCard search={settings.data.search} /> : <Skeleton className="h-40 w-full rounded-xl" />}
      </Section>
      <Section title={t('admin.condense.title')} description={t('admin.condense.description')}>
        {settings.data ? <CondenseSettingsCard condense={settings.data.condense} /> : <Skeleton className="h-40 w-full rounded-xl" />}
      </Section>
      <Section title={t('admin.compact.title')} description={t('admin.compact.description')}>
        {settings.data ? <CompactSettingsCard compact={settings.data.compact} /> : <Skeleton className="h-40 w-full rounded-xl" />}
      </Section>
    </div>
  );
}

/** A whole number from an input, or null if it is none in the range. */
function numberIn(value: string, min: number, max: number): number | null {
  const number = Number(value.trim());
  return value.trim() !== '' && Number.isFinite(number) && number >= min && number <= max ? number : null;
}

/**
 * Thresholds for condensing long chats: context size, share of the model's
 * window and pause.
 *
 * @param props - The stored thresholds.
 * @returns The card.
 */
function CondenseSettingsCard({ condense }: { condense: CondenseSettings }) {
  const t = useT();
  const queryClient = useQueryClient();
  const [enabled, setEnabled] = useState(condense.enabled);
  const [tokens, setTokens] = useState(String(condense.atTokens));
  const [share, setShare] = useState(String(Math.round(condense.atWindowShare * 100)));
  const [hours, setHours] = useState(String(condense.afterIdleHours));
  const [submitted, setSubmitted] = useState(false);
  const atTokens = numberIn(tokens, 10_000, 10_000_000);
  const atShare = numberIn(share, 10, 100);
  const afterHours = numberIn(hours, 0, 720);
  const next = { enabled, atTokens: atTokens ?? 0, atWindowShare: (atShare ?? 0) / 100, afterIdleHours: afterHours ?? 0 };
  const dirty =
    enabled !== condense.enabled ||
    next.atTokens !== condense.atTokens ||
    next.atWindowShare !== condense.atWindowShare ||
    next.afterIdleHours !== condense.afterIdleHours;

  const save = useMutation({
    mutationFn: () => api.settings.update({ condense: next }),
    onSuccess: (settings) => {
      queryClient.setQueryData(queryKeys.serverSettings, settings);
      setSubmitted(false);
      toast.success(t('admin.saved'));
    },
    onError: (error) => toast.error(errorMessage(t, error)),
  });

  function submit(event: FormEvent) {
    event.preventDefault();
    setSubmitted(true);
    if (atTokens !== null && atShare !== null && afterHours !== null) {
      save.mutate();
    }
  }

  return (
    <Card>
      <form onSubmit={submit} className="flex flex-col gap-4 p-4">
        <div className="flex items-start justify-between gap-4">
          <div className="flex min-w-0 flex-col gap-1">
            <span className="text-sm font-medium text-foreground">{t('admin.condense.enabled')}</span>
            <span className="text-sm text-muted-foreground">{t('admin.condense.enabledHint')}</span>
          </div>
          <Switch aria-label={t('admin.condense.enabled')} checked={enabled} onCheckedChange={setEnabled} />
        </div>
        {enabled ? (
          <div className="grid gap-3 sm:grid-cols-3">
            <Field
              label={t('admin.condense.atTokens')}
              hint={t('admin.condense.atTokensHint')}
              error={submitted && atTokens === null ? t('admin.condense.atTokensInvalid') : undefined}
            >
              {(control) => <Input {...control} inputMode="numeric" value={tokens} onChange={(event) => setTokens(event.target.value)} />}
            </Field>
            <Field
              label={t('admin.condense.atShare')}
              hint={t('admin.condense.atShareHint')}
              error={submitted && atShare === null ? t('admin.condense.atShareInvalid') : undefined}
            >
              {(control) => <Input {...control} inputMode="numeric" value={share} onChange={(event) => setShare(event.target.value)} />}
            </Field>
            <Field
              label={t('admin.condense.afterHours')}
              hint={t('admin.condense.afterHoursHint')}
              error={submitted && afterHours === null ? t('admin.condense.afterHoursInvalid') : undefined}
            >
              {(control) => <Input {...control} inputMode="decimal" value={hours} onChange={(event) => setHours(event.target.value)} />}
            </Field>
          </div>
        ) : null}
        <div>
          <Button type="submit" variant="primary" loading={save.isPending} disabled={!dirty}>
            {t('admin.condense.save')}
          </Button>
        </div>
      </form>
    </Card>
  );
}

/**
 * Thresholds for summarizing long chats: the free room in the context
 * window below which a chat is summarized, and how much of the latest turns
 * the model keeps reading word for word.
 *
 * @param props - The stored thresholds.
 * @returns The card.
 */
function CompactSettingsCard({ compact }: { compact: CompactSettings }) {
  const t = useT();
  const queryClient = useQueryClient();
  const [enabled, setEnabled] = useState(compact.enabled);
  const [reserve, setReserve] = useState(String(compact.reserveTokens));
  const [keep, setKeep] = useState(String(compact.keepRecentTokens));
  const [submitted, setSubmitted] = useState(false);
  const reserveTokens = numberIn(reserve, 1_000, 1_000_000);
  const keepRecentTokens = numberIn(keep, 0, 1_000_000);
  const next = { enabled, reserveTokens: reserveTokens ?? 0, keepRecentTokens: keepRecentTokens ?? 0 };
  const dirty = enabled !== compact.enabled || next.reserveTokens !== compact.reserveTokens || next.keepRecentTokens !== compact.keepRecentTokens;

  const save = useMutation({
    mutationFn: () => api.settings.update({ compact: next }),
    onSuccess: (settings) => {
      queryClient.setQueryData(queryKeys.serverSettings, settings);
      setSubmitted(false);
      toast.success(t('admin.saved'));
    },
    onError: (error) => toast.error(errorMessage(t, error)),
  });

  function submit(event: FormEvent) {
    event.preventDefault();
    setSubmitted(true);
    if (reserveTokens !== null && keepRecentTokens !== null) {
      save.mutate();
    }
  }

  return (
    <Card>
      <form onSubmit={submit} className="flex flex-col gap-4 p-4">
        <div className="flex items-start justify-between gap-4">
          <div className="flex min-w-0 flex-col gap-1">
            <span className="text-sm font-medium text-foreground">{t('admin.compact.enabled')}</span>
            <span className="text-sm text-muted-foreground">{t('admin.compact.enabledHint')}</span>
          </div>
          <Switch aria-label={t('admin.compact.enabled')} checked={enabled} onCheckedChange={setEnabled} />
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          <Field
            label={t('admin.compact.reserve')}
            hint={t('admin.compact.reserveHint')}
            error={submitted && reserveTokens === null ? t('admin.compact.reserveInvalid') : undefined}
          >
            {(control) => <Input {...control} inputMode="numeric" value={reserve} onChange={(event) => setReserve(event.target.value)} />}
          </Field>
          <Field
            label={t('admin.compact.keep')}
            hint={t('admin.compact.keepHint')}
            error={submitted && keepRecentTokens === null ? t('admin.compact.keepInvalid') : undefined}
          >
            {(control) => <Input {...control} inputMode="numeric" value={keep} onChange={(event) => setKeep(event.target.value)} />}
          </Field>
        </div>
        <div>
          <Button type="submit" variant="primary" loading={save.isPending} disabled={!dirty}>
            {t('admin.compact.save')}
          </Button>
        </div>
      </form>
    </Card>
  );
}

const PROVIDER_HINTS = {
  off: 'admin.search.offHint',
  searxng: 'admin.search.searxngHint',
  brave: 'admin.search.braveHint',
  tavily: 'admin.search.tavilyHint',
} as const satisfies Record<SearchProvider, MessageKey>;

/** What an unreachable provider most likely means, from the server's error code. */
function causeHint(t: TranslateFn, detail: string): string {
  if (detail === 'ENOTFOUND' || detail === 'EAI_AGAIN') {
    return t('admin.search.causes.notFound');
  }
  if (detail === 'ECONNREFUSED') {
    return t('admin.search.causes.refused');
  }
  if (detail === 'redirect') {
    return t('admin.search.causes.redirect');
  }
  return t('admin.search.causes.other', { detail });
}

/**
 * Provider, SearXNG address or API key of the web search, with a test
 * search against what is saved.
 *
 * @param props - The stored search settings.
 * @returns The card.
 */
function SearchSettingsCard({ search }: { search: SearchSettings }) {
  const t = useT();
  const queryClient = useQueryClient();
  const [provider, setProvider] = useState<SearchProvider>(search.provider);
  const [url, setUrl] = useState(search.searxngUrl ?? '');
  const [key, setKey] = useState('');
  const [helpers, setHelpers] = useState(search.helpers);
  const [submitted, setSubmitted] = useState(false);
  const [result, setResult] = useState<SearchTestResult | null>(null);
  const keyed = provider === 'brave' || provider === 'tavily' ? provider : null;
  const storedHint = keyed ? search.keys[keyed] : null;
  const urlError =
    provider === 'searxng' && !/^https?:\/\/\S+$/i.test(url.trim()) ? t('admin.search.urlInvalid') : undefined;
  const keyError = keyed && !storedHint && !key.trim() ? t('admin.search.keyMissing') : undefined;
  const modelError = (id: string) => (/^~?[\w.-]+\/[\w.:-]+$/.test(id.trim()) ? undefined : t('admin.search.modelInvalid'));
  const jevError = helpers.enabled ? modelError(helpers.jevModel) : undefined;
  const embeddingError = helpers.enabled ? modelError(helpers.embeddingModel) : undefined;
  const helpersChanged =
    helpers.enabled !== search.helpers.enabled ||
    helpers.jevModel.trim() !== search.helpers.jevModel ||
    helpers.embeddingModel.trim() !== search.helpers.embeddingModel;
  const dirty = provider !== search.provider || url.trim() !== (search.searxngUrl ?? '') || key.trim() !== '' || helpersChanged;

  const save = useMutation({
    mutationFn: () =>
      api.settings.update({
        search: {
          provider,
          ...(provider === 'searxng' ? { searxngUrl: url.trim() } : {}),
          ...(keyed && key.trim() ? { [`${keyed}Key`]: key.trim() } : {}),
          ...(helpersChanged
            ? { helpers: { enabled: helpers.enabled, jevModel: helpers.jevModel.trim(), embeddingModel: helpers.embeddingModel.trim() } }
            : {}),
        },
      }),
    onSuccess: (next) => {
      queryClient.setQueryData(queryKeys.serverSettings, next);
      void queryClient.invalidateQueries({ queryKey: queryKeys.publicSettings });
      setKey('');
      setSubmitted(false);
      setResult(null);
      toast.success(t('admin.saved'));
    },
    onError: (error) => toast.error(errorMessage(t, error)),
  });
  const test = useMutation({
    mutationFn: api.settings.testSearch,
    onSuccess: setResult,
    onError: (error) => toast.error(errorMessage(t, error)),
  });

  function submit(event: FormEvent) {
    event.preventDefault();
    setSubmitted(true);
    if (!urlError && !keyError && !jevError && !embeddingError) {
      save.mutate();
    }
  }

  return (
    <Card>
      <form onSubmit={submit} className="flex flex-col gap-4 p-4">
        <div className="flex flex-col gap-2">
          <span className="text-sm font-medium text-foreground">{t('admin.search.provider')}</span>
          <SegmentedControl
            label={t('admin.search.provider')}
            value={provider}
            onValueChange={(next) => {
              setProvider(next);
              setSubmitted(false);
            }}
            options={[
              { value: 'off', label: t('admin.search.off') },
              { value: 'searxng', label: 'SearXNG' },
              { value: 'brave', label: 'Brave' },
              { value: 'tavily', label: 'Tavily' },
            ]}
            className="self-start"
          />
          <p className="text-sm text-muted-foreground">{t(PROVIDER_HINTS[provider])}</p>
        </div>
        {provider === 'searxng' ? (
          <Field label={t('admin.search.url')} error={submitted ? urlError : undefined}>
            {(control) => (
              <Input
                {...control}
                type="url"
                inputMode="url"
                autoComplete="off"
                spellCheck={false}
                placeholder="http://searxng:8080"
                value={url}
                onChange={(event) => setUrl(event.target.value)}
              />
            )}
          </Field>
        ) : null}
        {keyed ? (
          <Field
            label={t('admin.search.key')}
            hint={storedHint ? t('admin.search.keyStored', { hint: storedHint }) : undefined}
            error={submitted ? keyError : undefined}
          >
            {(control) => (
              <Input
                {...control}
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={key}
                onChange={(event) => setKey(event.target.value)}
              />
            )}
          </Field>
        ) : null}
        <Separator />
        <div className="flex flex-col gap-3">
          <div className="flex items-start justify-between gap-4">
            <div className="flex min-w-0 flex-col gap-1">
              <span className="text-sm font-medium text-foreground">{t('admin.search.helpers')}</span>
              <span className="text-sm text-muted-foreground">{t('admin.search.helpersHint')}</span>
            </div>
            <Switch
              aria-label={t('admin.search.helpers')}
              checked={helpers.enabled}
              onCheckedChange={(enabled) => setHelpers({ ...helpers, enabled })}
            />
          </div>
          {helpers.enabled ? (
            <div className="grid gap-3 sm:grid-cols-2">
              <Field label={t('admin.search.jevModel')} hint={t('admin.search.jevModelHint')} error={submitted ? jevError : undefined}>
                {(control) => (
                  <Input
                    {...control}
                    autoComplete="off"
                    spellCheck={false}
                    value={helpers.jevModel}
                    onChange={(event) => setHelpers({ ...helpers, jevModel: event.target.value })}
                  />
                )}
              </Field>
              <Field
                label={t('admin.search.embeddingModel')}
                hint={t('admin.search.embeddingModelHint')}
                error={submitted ? embeddingError : undefined}
              >
                {(control) => (
                  <Input
                    {...control}
                    autoComplete="off"
                    spellCheck={false}
                    value={helpers.embeddingModel}
                    onChange={(event) => setHelpers({ ...helpers, embeddingModel: event.target.value })}
                  />
                )}
              </Field>
            </div>
          ) : null}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button type="submit" variant="primary" loading={save.isPending} disabled={!dirty}>
            {t('admin.search.save')}
          </Button>
          <Button
            variant="outline"
            loading={test.isPending}
            disabled={search.provider === 'off' || dirty}
            onClick={() => {
              setResult(null);
              test.mutate();
            }}
          >
            {t('admin.search.test')}
          </Button>
        </div>
        {search.provider === 'off' ? <p className="text-xs text-muted-foreground">{t('admin.search.notConfigured')}</p> : null}
        {result ? (
          <p
            role="status"
            className={cn(
              'flex items-start gap-2 rounded-lg px-3 py-2 text-sm',
              result.ok ? 'bg-muted text-foreground' : 'bg-destructive-subtle text-destructive',
            )}
          >
            {result.ok ? (
              <CircleCheck className="mt-0.5 size-4 shrink-0 text-success" aria-hidden="true" />
            ) : (
              <CircleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
            )}
            <span className="flex min-w-0 flex-col gap-1 break-words">
              <span>
                {result.ok
                  ? t('admin.search.testOk', { count: result.count })
                  : t('admin.search.testFailed', { reason: t(`webSearch.errors.${result.code}`, { detail: result.detail ?? '' }) })}
              </span>
              {!result.ok && result.code === 'network' && result.detail ? <span>{causeHint(t, result.detail)}</span> : null}
            </span>
          </p>
        ) : null}
      </form>
    </Card>
  );
}
