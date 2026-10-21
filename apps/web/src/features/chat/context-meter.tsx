import { Tooltip } from '@/components/ui/menu.tsx';
import type { ChatMessage } from '@/lib/api.ts';
import { usePreferences } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

/** From this share of the context window on, the ring warns. */
const WARN_PERCENT = 80;

/**
 * Formats a cost in US dollars; small amounts keep more digits, so a chat
 * that costs a fraction of a cent does not read as free.
 *
 * @param locale - UI language.
 * @param cost - Dollars.
 * @returns The formatted amount.
 */
export function formatCost(locale: string, cost: number): string {
  return new Intl.NumberFormat(locale, {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits: cost < 0.01 ? 4 : 2,
  }).format(cost);
}

function formatTokens(locale: string, tokens: number): string {
  return new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 1 }).format(tokens);
}

/**
 * Totals of a conversation: the context after the last reply and the cost
 * of all replies with a known price.
 *
 * @param messages - The conversation.
 * @returns Context and cost, or null before the first reply with usage.
 */
export function conversationUsage(messages: ChatMessage[]) {
  const replies = messages.filter((message) => message.role === 'assistant' && message.metadata?.usage);
  const last = replies.at(-1)?.metadata?.usage;
  if (!last) {
    return null;
  }
  const priced = replies.flatMap((message) => (message.metadata?.usage?.cost === undefined ? [] : [message.metadata.usage.cost]));
  const total = (pick: (usage: NonNullable<NonNullable<ChatMessage['metadata']>['usage']>) => number | undefined) =>
    replies.reduce((sum, message) => sum + (message.metadata?.usage ? (pick(message.metadata.usage) ?? 0) : 0), 0);
  return {
    contextTokens: last.contextTokens ?? (last.inputTokens ?? 0) + (last.outputTokens ?? 0),
    contextWindow: last.contextWindow,
    cost: priced.length > 0 ? priced.reduce((sum, cost) => sum + cost, 0) : undefined,
    unpriced: replies.length - priced.length,
    inputTokens: total((usage) => usage.inputTokens),
    cachedInputTokens: total((usage) => usage.cachedInputTokens),
    outputTokens: total((usage) => usage.outputTokens),
    reasoningTokens: total((usage) => usage.reasoningTokens),
  };
}

/**
 * How full the model's context is and what the chat has cost so far, shown
 * in the composer; details in the tooltip.
 *
 * @param props - The conversation's messages.
 * @returns The meter, or nothing before the first reply.
 */
export function ContextMeter({ messages }: { messages: ChatMessage[] }) {
  const { t, locale } = usePreferences();
  const usage = conversationUsage(messages);
  if (!usage) {
    return null;
  }
  const { contextTokens, contextWindow, cost, unpriced } = usage;
  const percent = contextWindow ? Math.min(100, Math.round((contextTokens / contextWindow) * 100)) : null;
  const used = formatTokens(locale, contextTokens);
  // The tooltip has room for exact numbers; the button shows the short form.
  const exact = new Intl.NumberFormat(locale);
  const details = [
    percent === null
      ? t('chat.contextTokens', { used: exact.format(contextTokens) })
      : t('chat.contextDetails', { used: exact.format(contextTokens), window: exact.format(contextWindow ?? 0), percent }),
    cost === undefined ? t('chat.costUnknown') : t('chat.costTotal', { cost: formatCost(locale, cost) }),
    ...(cost !== undefined && unpriced > 0 ? [t('chat.costPartial', { count: unpriced })] : []),
    t('chat.tokenTotals', {
      input: exact.format(usage.inputTokens),
      cached: exact.format(usage.cachedInputTokens),
      output: exact.format(usage.outputTokens),
      reasoning: exact.format(usage.reasoningTokens),
    }),
    t('chat.costSource'),
  ];
  const warn = percent !== null && percent >= WARN_PERCENT;

  return (
    <Tooltip content={<span className="flex max-w-72 flex-col gap-1">{details.map((line) => <span key={line}>{line}</span>)}</span>}>
      <button
        type="button"
        aria-label={details.join(' ')}
        className="flex shrink-0 items-center gap-1.5 rounded-md px-1.5 py-1 text-xs tabular-nums text-muted-foreground transition-colors duration-(--duration-instant) ease-standard hover:text-foreground focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
      >
        {percent === null ? null : <Ring percent={percent} warn={warn} />}
        <span className={cn(warn && 'text-destructive')}>{percent === null ? used : `${percent} %`}</span>
        {cost === undefined ? null : <span className="max-sm:hidden">· {formatCost(locale, cost)}</span>}
      </button>
    </Tooltip>
  );
}

function Ring({ percent, warn }: { percent: number; warn: boolean }) {
  // Circumference of r = 6 in a 16 × 16 box.
  const circumference = 2 * Math.PI * 6;
  return (
    <svg viewBox="0 0 16 16" className="size-4 -rotate-90" aria-hidden="true">
      <circle cx="8" cy="8" r="6" fill="none" strokeWidth="2.5" className="stroke-border" />
      <circle
        cx="8"
        cy="8"
        r="6"
        fill="none"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeDasharray={`${(percent / 100) * circumference} ${circumference}`}
        className={warn ? 'stroke-destructive' : 'stroke-primary'}
      />
    </svg>
  );
}
