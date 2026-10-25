import { FoldVertical } from 'lucide-react';
import { useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Popover, PopoverContent, PopoverTrigger, Tooltip } from '@/components/ui/menu.tsx';
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
    // Every priced reply as billed by the provider, none estimated.
    billed: priced.length > 0 && replies.every((message) => message.metadata?.usage?.cost === undefined || message.metadata.usage.billed === true),
    inputTokens: total((usage) => usage.inputTokens),
    cachedInputTokens: total((usage) => usage.cachedInputTokens),
    outputTokens: total((usage) => usage.outputTokens),
    reasoningTokens: total((usage) => usage.reasoningTokens),
  };
}

/**
 * Props of {@link ContextMeter}.
 */
export interface ContextMeterProps {
  /** The conversation's messages. */
  messages: ChatMessage[];
  /**
   * Summarizes the chat now; without it (linked chats) the meter only shows
   * the numbers.
   */
  onCompact?: () => Promise<void>;
  /** A reply is being written; summarizing waits for it. */
  busy?: boolean;
}

/**
 * How full the model's context is and what the chat has cost so far, shown
 * in the composer; details in the tooltip. A click opens the details with
 * a button that summarizes the chat now.
 *
 * @param props - See {@link ContextMeterProps}.
 * @returns The meter, or nothing before the first reply.
 */
export function ContextMeter({ messages, onCompact, busy = false }: ContextMeterProps) {
  const { t, locale } = usePreferences();
  const [open, setOpen] = useState(false);
  const [compacting, setCompacting] = useState(false);
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
    usage.billed ? t('chat.costBilled') : t('chat.costSource'),
  ];
  const warn = percent !== null && percent >= WARN_PERCENT;
  const lines = <span className="flex max-w-72 flex-col gap-1">{details.map((line) => <span key={line}>{line}</span>)}</span>;
  const meter = (
    <button
      type="button"
      aria-label={details.join(' ')}
      className="flex shrink-0 items-center gap-1.5 rounded-md px-1.5 py-1 text-xs tabular-nums text-muted-foreground transition-colors duration-(--duration-instant) ease-standard hover:text-foreground focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
    >
      {percent === null ? null : <Ring percent={percent} warn={warn} />}
      <span className={cn(warn && 'text-destructive')}>{percent === null ? used : `${percent} %`}</span>
      {cost === undefined ? null : <span className="max-sm:hidden">· {formatCost(locale, cost)}</span>}
    </button>
  );
  if (!onCompact) {
    return <Tooltip content={lines}>{meter}</Tooltip>;
  }
  // A summary covers everything but the latest turn, so it needs two.
  const turns = messages.filter((message) => message.role === 'user').length;

  async function compact(run: () => Promise<void>) {
    setCompacting(true);
    try {
      await run();
      setOpen(false);
    } finally {
      setCompacting(false);
    }
  }

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <Tooltip content={lines}>
        <PopoverTrigger asChild>{meter}</PopoverTrigger>
      </Tooltip>
      <PopoverContent side="top" align="end" className="flex w-[min(var(--spacing)*80,calc(100vw-var(--spacing)*8))] flex-col gap-3 p-4">
        <div className="flex flex-col gap-1 text-xs text-muted-foreground">
          {details.map((line) => (
            <span key={line}>{line}</span>
          ))}
        </div>
        <Button
          variant="secondary"
          size="sm"
          loading={compacting}
          disabled={busy || turns < 2}
          onClick={() => void compact(onCompact)}
        >
          <FoldVertical />
          {t('chat.compact.now')}
        </Button>
        <p className="text-xs text-muted-foreground">{turns < 2 ? t('chat.compact.tooShort') : t('chat.compact.hint')}</p>
      </PopoverContent>
    </Popover>
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
