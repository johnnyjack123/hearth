import type { OpenRouterHost, OpenRouterHosts } from '@harness/shared';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check } from 'lucide-react';
import type { ReactNode } from 'react';
import { Badge } from '@/components/ui/display.tsx';
import type { TranslateFn } from '@/i18n/index.ts';
import { api } from '@/lib/api.ts';
import { usePreferences } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';
import { formatDollars } from './usage-details.tsx';

/** The OpenRouter host of a model, as the chat options show it. */
export interface HostOption {
  /** The pinned host's name, or null when calls go to the cheapest host. */
  pinned: string | null;
  /** The page with the hosts to pick from. */
  details: ReactNode;
}

function hostsKey(model: string) {
  return ['openrouter-hosts', model] as const;
}

/**
 * The host a model of OpenRouter runs on: the cheapest (OpenRouter's list,
 * the chat keeps to one host for its cache), or one the user pinned it to
 * for all chats and agents, say to keep data away from a country.
 *
 * @param model - The chat's model id when its credential is OpenRouter, else null.
 * @returns The option, or null when there is no choice (another provider, a model variant).
 */
export function useOpenRouterHost(model: string | null): HostOption | null {
  const queryClient = useQueryClient();
  const hosts = useQuery({
    queryKey: hostsKey(model ?? ''),
    queryFn: () => api.openrouter.hosts(model ?? ''),
    enabled: model !== null,
    // Prices and uptime move; the list on the server is ten minutes old at most.
    staleTime: 5 * 60_000,
  });
  const pin = useMutation({
    mutationFn: (host: string | null) => api.openrouter.pin(model ?? '', host),
    onSuccess: (data) => queryClient.setQueryData(hostsKey(model ?? ''), data),
  });
  if (model === null || (hosts.data && hosts.data.hosts === null && hosts.data.pinned === null)) {
    return null;
  }
  const pinned = hosts.data?.pinned ?? null;
  return {
    pinned: pinned === null ? null : (hosts.data?.hosts?.find((host) => host.slug === pinned)?.name ?? pinned),
    details: (
      <HostList
        data={hosts.data}
        loading={hosts.isPending}
        busy={pin.isPending}
        failed={pin.isError}
        onPick={(host) => {
          pin.mutate(host);
        }}
      />
    ),
  };
}

/**
 * The hosts of a model to pick from: the cheapest automatically, or one
 * fixed host with where the company sits and what it charges.
 *
 * @param props - The list, whether it loads or a pick is being saved, and what a pick does.
 * @returns The list.
 */
export function HostList({
  data,
  loading,
  busy,
  failed,
  onPick,
}: {
  data: OpenRouterHosts | undefined;
  loading: boolean;
  busy: boolean;
  failed: boolean;
  onPick: (host: string | null) => void;
}) {
  const { t, locale } = usePreferences();
  const pinned = data?.pinned ?? null;
  const hosts = data?.hosts ?? [];
  // A pinned host OpenRouter no longer lists stays visible, so it can be undone.
  const missing = pinned !== null && !hosts.some((host) => host.slug === pinned);
  return (
    <div className="flex flex-col gap-2">
      <p className="text-sm font-medium text-foreground">{t('chat.host.title')}</p>
      <p className="text-xs text-muted-foreground">{t('chat.host.intro')}</p>
      {failed ? (
        <p role="alert" className="text-xs text-destructive">
          {t('chat.host.failed')}
        </p>
      ) : null}
      <div role="radiogroup" aria-label={t('chat.host.title')} aria-busy={busy || loading} className="flex flex-col">
        <Choice selected={pinned === null} disabled={busy} onSelect={() => onPick(null)} title={t('chat.host.auto')} lines={[t('chat.host.autoHint')]} />
        {missing ? <Choice selected disabled={busy} onSelect={() => undefined} title={pinned ?? ''} lines={[t('chat.host.gone')]} /> : null}
        {loading ? <p className="px-2 py-2 text-xs text-muted-foreground">{t('chat.host.loading')}</p> : null}
        {!loading && data?.hosts === null ? <p className="px-2 py-2 text-xs text-muted-foreground">{t('chat.host.unavailable')}</p> : null}
        {hosts.map((host) => (
          <Choice
            key={host.slug}
            selected={pinned === host.slug}
            disabled={busy}
            onSelect={() => onPick(host.slug)}
            title={host.name}
            badge={host.available ? null : t('chat.host.down')}
            lines={[where(host, locale, t), prices(host, locale, t)].filter((line): line is string => line !== null)}
          />
        ))}
      </div>
    </div>
  );
}

/** Seat and data centers, e.g. "USA" or "Sitz: Singapur · Rechenzentren: USA". */
function where(host: OpenRouterHost, locale: string, t: TranslateFn): string | null {
  const names = new Intl.DisplayNames([locale], { type: 'region' });
  const country = (code: string) => names.of(code) ?? code;
  const seat = host.headquarters === null ? null : country(host.headquarters);
  const centers = host.datacenters.filter((code) => code !== host.headquarters).map(country);
  const quantization = host.quantizations.length > 0 ? host.quantizations.join(', ') : null;
  const parts = [
    seat === null ? t('chat.host.seatUnknown') : centers.length > 0 ? t('chat.host.seat', { country: seat }) : seat,
    centers.length > 0 ? t('chat.host.datacenters', { countries: centers.join(', ') }) : null,
    quantization,
  ];
  return parts.filter(Boolean).join(' · ');
}

function prices(host: OpenRouterHost, locale: string, t: TranslateFn): string | null {
  const rates = host.rates;
  if (!rates) {
    return null;
  }
  return t('chat.host.prices', {
    input: formatDollars(locale, rates.input),
    cacheRead: rates.cacheRead === undefined ? '–' : formatDollars(locale, rates.cacheRead),
    output: formatDollars(locale, rates.output),
  });
}

function Choice({
  selected,
  disabled,
  onSelect,
  title,
  badge = null,
  lines,
}: {
  selected: boolean;
  disabled: boolean;
  onSelect: () => void;
  title: string;
  badge?: string | null;
  lines: string[];
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      disabled={disabled}
      onClick={onSelect}
      className={cn(
        'flex w-full items-start gap-2 rounded-md px-2 py-2 text-start transition-colors duration-(--duration-instant) ease-standard hover:bg-accent disabled:opacity-60',
        'focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring',
      )}
    >
      <Check className={cn('mt-0.5 size-4 shrink-0 text-primary', !selected && 'invisible')} aria-hidden="true" />
      <span className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="flex items-center gap-2 text-sm font-medium text-foreground">
          <span className="truncate">{title}</span>
          {badge ? (
            <Badge variant="destructive" className="shrink-0">
              {badge}
            </Badge>
          ) : null}
        </span>
        {lines.map((line) => (
          <span key={line} className="text-xs text-muted-foreground">
            {line}
          </span>
        ))}
      </span>
    </button>
  );
}
