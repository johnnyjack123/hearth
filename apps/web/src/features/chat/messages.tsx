import {
  type ApprovalDecision,
  CALCULATOR_TOOL,
  type Compaction,
  FORGET_TOOL,
  type Inspection,
  REMEMBER_TOOL,
  WEB_FETCH_TOOL,
  WEB_SEARCH_TOOL,
} from '@hearth/shared';
import { Brain, Check, ChevronRight, CircleAlert, Container, Copy, FoldVertical, Receipt, RefreshCw } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { lazy, Suspense, useMemo, useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Tooltip } from '@/components/ui/menu.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import type { MessageKey, TranslateFn } from '@/i18n/index.ts';
import type { ChatMessage } from '@/lib/api.ts';
import { riseIn, transitions } from '@/lib/motion.ts';
import { usePreferences, useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';
import { ChecklistUpdate, PROGRESS_TOOL, readChecklist } from './agent-checklist.tsx';
import { MessageFiles } from './attachments.tsx';
import { AgentNotice, AgentProgress, ApprovalCard, ToolCall } from './agent-parts.tsx';
import { CalculatorCall } from './calculator-call.tsx';
import { DocumentRead } from './document-read.tsx';
import { MemoryCall, memoryCallView } from './memory-call.tsx';
import { TakeoverCard } from './takeover-card.tsx';
import { formatCost } from './context-meter.tsx';
import { formatRate, outputRate, UsageDetails } from './usage-details.tsx';
import { WebFetchCall } from './web-fetch.tsx';
import { type KnownSource, knownSources, sourcesSignature, unsourcedLinks } from './citations.ts';
import { readPages, Sources, UnsourcedLinks, WebSearchCall } from './web-search.tsx';

const Markdown = lazy(() => import('./markdown.tsx').then((module) => ({ default: module.Markdown })));

/**
 * Concatenates the text parts of a message.
 *
 * @param message - Chat message.
 * @returns Plain text without reasoning or tool parts.
 */
export function messageText(message: ChatMessage): string {
  return message.parts.map((part) => (part.type === 'text' ? part.text : '')).join('');
}

/**
 * Props of {@link MessageList}.
 */
export interface MessageListProps {
  /** Messages in order. */
  messages: ChatMessage[];
  /** useChat status. */
  status: 'submitted' | 'streaming' | 'ready' | 'error';
  /** Error of the last request, if any. */
  error: Error | undefined;
  /** Ids rendered at mount; they skip the entrance animation. */
  initialIds: ReadonlySet<string>;
  /** Re-runs the last request after an error or for a new reply. */
  onRegenerate: () => void;
  /** Answers a question of the gateway in a linked chat. */
  onApprove?: (id: number, decision: ApprovalDecision) => Promise<void>;
  /** Asks Jev about the prompt injection warning of a tool call in a stored reply. */
  onAssess?: (messageId: string, toolCallId: string) => Promise<void>;
  /** Loads the whole content behind the prompt injection warning of a tool call in a stored reply. */
  onInspect?: (messageId: string, toolCallId: string) => Promise<Inspection>;
}

/**
 * All messages of a conversation plus the typing indicator and error state.
 *
 * @param props - See {@link MessageListProps}.
 * @returns The message column.
 */
export function MessageList({ messages, status, error, initialIds, onRegenerate, onApprove, onAssess, onInspect }: MessageListProps) {
  const lastAssistantId = messages.findLast((message) => message.role === 'assistant')?.id;
  const busy = status === 'submitted' || status === 'streaming';
  // Links of a reply are checked against everything the chat read or found.
  // The map stays the same while a reply streams text, so its Markdown
  // blocks are not all rendered again for every chunk.
  const current = knownSources(messages);
  const signature = sourcesSignature(current);
  const known = useMemo(() => current, [signature]);
  // A summary sits where it ends, wherever the chat stored it.
  const summaries = finishedCompactions(messages);

  return (
    <div className="mx-auto flex w-full max-w-(--content-width) flex-col gap-8 px-4 py-8" aria-live="polite" aria-busy={busy}>
      {messages.map((message, index) => {
        const animate = !initialIds.has(message.id);
        const summary = summaries.get(message.id);
        const divider = summary ? <CompactionDivider key={`${message.id}-summary`} compaction={summary} /> : null;
        if (message.role === 'user') {
          return [<UserMessage key={message.id} message={message} animate={animate} />, divider];
        }
        if (message.role === 'assistant') {
          const isLast = index === messages.length - 1;
          return [
            <AssistantMessage
              key={message.id}
              message={message}
              animate={animate}
              streaming={isLast && status === 'streaming'}
              canRegenerate={message.id === lastAssistantId && !busy}
              onRegenerate={onRegenerate}
              onApprove={onApprove}
              onAssess={onAssess}
              onInspect={onInspect}
              known={known}
            />,
            divider,
          ];
        }
        return null;
      })}
      <AnimatePresence>{status === 'submitted' ? <TypingIndicator key="typing" /> : null}</AnimatePresence>
      {error && !busy ? <StreamError error={error} onRetry={onRegenerate} /> : null}
    </div>
  );
}

function UserMessage({ message, animate }: { message: ChatMessage; animate: boolean }) {
  const text = messageText(message);
  const files = message.parts.filter((part) => part.type === 'file');
  return (
    // Full width, so the bubble's 80 % refer to the chat column: a column
    // as wide as its content made the bubble 80 % of its own text and
    // stacked short messages letter by letter.
    <motion.div variants={riseIn} initial={animate ? 'initial' : false} animate="animate" className="flex w-full flex-col items-end gap-2">
      <MessageFiles parts={files} />
      {text ? (
        <div className="max-w-4/5 whitespace-pre-wrap break-words rounded-2xl rounded-ee-sm bg-bubble px-4 py-2.5 text-base leading-7 text-bubble-foreground">
          {text}
        </div>
      ) : null}
    </motion.div>
  );
}

function AssistantMessage({
  message,
  animate,
  streaming,
  canRegenerate,
  onRegenerate,
  onApprove,
  onAssess,
  onInspect,
  known,
}: {
  message: ChatMessage;
  animate: boolean;
  streaming: boolean;
  canRegenerate: boolean;
  onRegenerate: () => void;
  onApprove: ((id: number, decision: ApprovalDecision) => Promise<void>) | undefined;
  onAssess: ((messageId: string, toolCallId: string) => Promise<void>) | undefined;
  onInspect: ((messageId: string, toolCallId: string) => Promise<Inspection>) | undefined;
  known: ReadonlyMap<string, KnownSource>;
}) {
  // Only stored replies can be assessed; a streaming one is not saved yet.
  const assess = (toolCallId: string) => (onAssess && !streaming ? () => onAssess(message.id, toolCallId) : undefined);
  const inspect = (toolCallId: string) => (onInspect && !streaming ? () => onInspect(message.id, toolCallId) : undefined);
  // The search blocks mark these hits, the sources list them.
  const pages = readPages(message);
  const harness = message.metadata?.agent?.harness ?? 'pi';
  const toolMs = message.metadata?.toolMs;
  return (
    <motion.div variants={riseIn} initial={animate ? 'initial' : false} animate="animate" className="group flex min-w-0 flex-col gap-3">
      {message.metadata?.condensed ? <CondensedNote /> : null}
      {message.parts.map((part, index) => {
        if (part.type === 'reasoning' && part.text.trim()) {
          return <Reasoning key={index} text={part.text} streaming={streaming && part.state === 'streaming'} />;
        }
        if (
          ((part.type === 'dynamic-tool' && part.toolName === PROGRESS_TOOL) || part.type === `tool-${PROGRESS_TOOL}`) &&
          part.state !== 'output-error'
        ) {
          const items = readChecklist(part.input);
          if (items) {
            return <ChecklistUpdate key={part.toolCallId} items={items} live={streaming} />;
          }
        }
        if (part.type === 'dynamic-tool' && (part.toolName === REMEMBER_TOOL || part.toolName === FORGET_TOOL)) {
          return <MemoryCall key={part.toolCallId} {...memoryCallView(part.toolName, part, streaming)} />;
        }
        if (part.type === 'dynamic-tool' && part.toolName === 'read_document') {
          return <DocumentRead key={part.toolCallId} part={part} streaming={streaming} />;
        }
        if (part.type === 'dynamic-tool') {
          return (
            <ToolCall
              key={part.toolCallId}
              part={part}
              streaming={streaming}
              ms={toolMs?.[part.toolCallId]}
              onAssess={assess(part.toolCallId)}
              onInspect={inspect(part.toolCallId)}
            />
          );
        }
        if (part.type === `tool-${WEB_FETCH_TOOL}`) {
          return (
            <WebFetchCall
              key={part.toolCallId}
              part={part}
              streaming={streaming}
              ms={toolMs?.[part.toolCallId]}
              onAssess={assess(part.toolCallId)}
              onInspect={inspect(part.toolCallId)}
            />
          );
        }
        if (part.type === `tool-${REMEMBER_TOOL}` || part.type === `tool-${FORGET_TOOL}`) {
          return <MemoryCall key={part.toolCallId} {...memoryCallView(part.type.slice('tool-'.length), part, streaming)} />;
        }
        if (part.type === 'tool-read_document') {
          return <DocumentRead key={part.toolCallId} part={part} streaming={streaming} />;
        }
        if (part.type === `tool-${CALCULATOR_TOOL}`) {
          return <CalculatorCall key={part.toolCallId} part={part} streaming={streaming} ms={toolMs?.[part.toolCallId]} />;
        }
        if (part.type === `tool-${WEB_SEARCH_TOOL}`) {
          return (
            <WebSearchCall
              key={part.toolCallId}
              part={part}
              streaming={streaming}
              read={pages}
              ms={toolMs?.[part.toolCallId]}
              onAssess={assess(part.toolCallId)}
              onInspect={inspect(part.toolCallId)}
            />
          );
        }
        if (part.type === 'data-notice') {
          return <AgentNotice key={index} notice={part.data} />;
        }
        if (part.type === 'data-progress') {
          return <AgentProgress key={part.id ?? index} progress={part.data} />;
        }
        if (part.type === 'data-approval') {
          return <ApprovalCard key={part.id ?? index} approval={part.data} live={streaming} onDecide={onApprove} />;
        }
        if (part.type === 'data-takeover') {
          return <TakeoverCard key={part.id ?? index} report={part.data} />;
        }
        if (part.type === 'data-compaction') {
          if (part.data.state !== 'done') {
            return <CompactionStatus key={part.id ?? index} compaction={part.data} harness={harness} />;
          }
          // The chat's own summary shows as a divider where it ends; an
          // agent's covers its own history, so it stays where it happened.
          return part.data.agent ? <CompactionDivider key={part.id ?? index} compaction={part.data} harness={harness} /> : null;
        }
        if (part.type === 'text') {
          return (
            <Suspense key={index} fallback={<p className="whitespace-pre-wrap text-base leading-7">{part.text}</p>}>
              <Markdown animating={streaming} sources={known}>
                {part.text}
              </Markdown>
            </Suspense>
          );
        }
        return null;
      })}
      {!streaming ? <Sources pages={pages} /> : null}
      {!streaming ? <UnsourcedLinks links={unsourcedLinks(message, known)} /> : null}
      {!streaming ? <MessageActions message={message} canRegenerate={canRegenerate} onRegenerate={onRegenerate} /> : null}
    </motion.div>
  );
}

/**
 * Finished summaries by the id of the last message they cover; a newer one
 * ending at the same place wins.
 *
 * @param messages - The conversation.
 * @returns The summaries.
 */
function finishedCompactions(messages: ChatMessage[]): Map<string, Compaction> {
  const found = new Map<string, Compaction>();
  for (const message of messages) {
    for (const part of message.parts) {
      if (part.type === 'data-compaction' && part.data.state === 'done' && part.data.through) {
        found.set(part.data.through, part.data);
      }
    }
  }
  return found;
}

/**
 * Marks where a summary ends: the model reads it instead of everything
 * above. The summary itself unfolds on demand.
 */
function CompactionDivider({ compaction, harness = 'pi' }: { compaction: Compaction; harness?: string }) {
  const { t, locale } = usePreferences();
  const [open, setOpen] = useState(false);
  const tokens = new Intl.NumberFormat(locale);
  return (
    <div className="flex flex-col gap-3" role="note">
      <div className="flex items-center gap-3 text-xs text-muted-foreground">
        <span className="h-px flex-1 bg-border" aria-hidden="true" />
        <FoldVertical className="size-3.5 shrink-0" aria-hidden="true" />
        <span className="text-center">
          {compaction.agent ? t('chat.compaction.agentDivider', { harness }) : t('chat.compaction.divider')} ·{' '}
          {t(compaction.trigger === 'manual' ? 'chat.compaction.manual' : 'chat.compaction.auto')}
          {compaction.agent && compaction.tokens !== undefined && compaction.tokensAfter !== undefined
            ? ` · ${t('chat.compaction.tokens', { before: tokens.format(compaction.tokens), after: tokens.format(compaction.tokensAfter) })}`
            : null}
        </span>
        <button
          type="button"
          onClick={() => setOpen((value) => !value)}
          aria-expanded={open}
          className="inline-flex shrink-0 items-center gap-1 rounded-md text-foreground underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
        >
          {open ? t('chat.compaction.hide') : t('chat.compaction.show')}
          <ChevronRight
            className={cn('size-3.5 transition-transform duration-(--duration-fast) ease-standard', open && 'rotate-90')}
            aria-hidden="true"
          />
        </button>
        <span className="h-px flex-1 bg-border" aria-hidden="true" />
      </div>
      <AnimatePresence initial={false}>
        {open && compaction.summary ? (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1, transition: transitions.enter }}
            exit={{ opacity: 0, transition: transitions.exit }}
            className="rounded-xl bg-muted px-4 py-3 text-sm"
          >
            <Suspense fallback={<p className="whitespace-pre-wrap">{compaction.summary}</p>}>
              <Markdown animating={false}>{compaction.summary}</Markdown>
            </Suspense>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}

/** A summary being written (by the chat or its agent), or one that failed. */
function CompactionStatus({ compaction, harness }: { compaction: Compaction; harness: string }) {
  const t = useT();
  if (compaction.state === 'running') {
    return (
      <p className="flex items-center gap-2 text-xs text-muted-foreground" role="status">
        <Spinner className="size-3.5" />
        {compaction.agent ? t('chat.compaction.agentRunning', { harness }) : t('chat.compaction.running')}
      </p>
    );
  }
  const error = compaction.error ?? '';
  return (
    <p className="flex items-start gap-2 text-xs text-muted-foreground">
      <CircleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
      {compaction.agent ? t('chat.compaction.agentFailed', { harness, error }) : t('chat.compaction.failed', { error })}
    </p>
  );
}

/** Says why the model may load a page again that it read earlier in the chat. */
function CondensedNote() {
  const t = useT();
  return (
    <p className="flex items-start gap-2 text-xs text-muted-foreground">
      <FoldVertical className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
      {t('chat.condensed')}
    </p>
  );
}

/**
 * The model's thinking. While it writes, the block follows the browser's
 * preference; a click opens or closes it at any time and sets the
 * preference for the next ones. Finished thinking stays closed unless
 * opened.
 */
function Reasoning({ text, streaming }: { text: string; streaming: boolean }) {
  const { t, showThinking, setShowThinking } = usePreferences();
  const [chosen, setChosen] = useState<boolean | null>(null);
  const expanded = chosen ?? (streaming && showThinking);
  return (
    <div className="flex flex-col gap-2">
      <button
        type="button"
        onClick={() => {
          setChosen(!expanded);
          if (streaming) {
            setShowThinking(!expanded);
          }
        }}
        aria-expanded={expanded}
        className="inline-flex w-fit items-center gap-1.5 rounded-md text-sm text-muted-foreground transition-colors hover:text-foreground"
      >
        <Brain className={cn('size-4', streaming && 'animate-skeleton')} aria-hidden="true" />
        {streaming ? t('chat.thinking') : t('chat.reasoning')}
        <ChevronRight
          className={cn('size-3.5 transition-transform duration-(--duration-fast) ease-standard', expanded && 'rotate-90')}
          aria-hidden="true"
        />
      </button>
      <AnimatePresence initial={false}>
        {expanded ? (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1, transition: transitions.enter }}
            exit={{ opacity: 0, transition: transitions.exit }}
            className="whitespace-pre-wrap border-s-2 border-border ps-4 text-sm leading-6 text-muted-foreground"
          >
            {text}
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}

function MessageActions({
  message,
  canRegenerate,
  onRegenerate,
}: {
  message: ChatMessage;
  canRegenerate: boolean;
  onRegenerate: () => void;
}) {
  const { t, locale } = usePreferences();
  const [copied, setCopied] = useState(false);
  const [showUsage, setShowUsage] = useState(false);
  const usage = message.metadata?.usage;
  const rate = outputRate(usage?.calls ?? []);
  // Where OpenRouter sent the calls; usually one host, the order keeps the cache there.
  const hosts = [...new Set((usage?.calls ?? []).flatMap((call) => (call.host ? [call.host] : [])))];

  async function copy() {
    await navigator.clipboard.writeText(messageText(message));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }

  return (
    <>
      <div className="flex items-center gap-1 text-muted-foreground opacity-100 transition-opacity duration-(--duration-fast) md:opacity-0 md:group-hover:opacity-100 md:focus-within:opacity-100 md:group-last:opacity-100">
        <Tooltip content={copied ? t('common.copied') : t('chat.copyMessage')}>
          <Button variant="ghost" size="icon-sm" aria-label={t('chat.copyMessage')} onClick={() => void copy()}>
            {copied ? <Check className="text-success" /> : <Copy />}
          </Button>
        </Tooltip>
        {canRegenerate ? (
          <Tooltip content={t('chat.regenerate')}>
            <Button variant="ghost" size="icon-sm" aria-label={t('chat.regenerate')} onClick={onRegenerate}>
              <RefreshCw />
            </Button>
          </Tooltip>
        ) : null}
        {message.metadata?.agent ? (
          <span className="ms-1 inline-flex min-w-0 items-center gap-1 truncate text-xs">
            <Container className="size-3.5 shrink-0" aria-hidden="true" />
            {t('agent.linkedTo', { harness: message.metadata.agent.harness, project: message.metadata.agent.project })}
          </span>
        ) : null}
        {message.metadata?.modelId ? (
          <span className="ms-1 truncate text-xs">
            {message.metadata.modelId}
            {usage?.inputTokens !== undefined && usage.outputTokens !== undefined
              ? ` · ${t('chat.usage', { input: usage.inputTokens, output: usage.outputTokens })}`
              : null}
            {usage?.cost !== undefined ? ` · ${formatCost(locale, usage.cost)}` : null}
            {rate !== null ? ` · ${t('chat.rate', { rate: formatRate(locale, rate) })}` : null}
            {hosts.length > 0 ? ` · ${t('chat.via', { hosts: hosts.join(', ') })}` : null}
          </span>
        ) : null}
        {usage ? (
          <Tooltip content={showUsage ? t('chat.usageDetails.hide') : t('chat.usageDetails.show')}>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={showUsage ? t('chat.usageDetails.hide') : t('chat.usageDetails.show')}
              aria-expanded={showUsage}
              onClick={() => setShowUsage(!showUsage)}
            >
              <Receipt />
            </Button>
          </Tooltip>
        ) : null}
      </div>
      <AnimatePresence initial={false}>
        {usage && showUsage ? (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1, transition: transitions.enter }}
            exit={{ opacity: 0, transition: transitions.exit }}
          >
            <UsageDetails usage={usage} agent={message.metadata?.agent !== undefined} modelId={message.metadata?.modelId} />
          </motion.div>
        ) : null}
      </AnimatePresence>
    </>
  );
}

function TypingIndicator() {
  const t = useT();
  return (
    <motion.div
      variants={riseIn}
      initial="initial"
      animate="animate"
      exit="exit"
      className="flex items-center gap-1.5 py-2"
      role="status"
      aria-label={t('chat.thinking')}
    >
      {[0, 1, 2].map((dot) => (
        <span
          key={dot}
          className="size-2 animate-dot-pulse rounded-full bg-muted-foreground"
          style={{ animationDelay: `calc(var(--duration-fast) * ${dot})` }}
        />
      ))}
    </motion.div>
  );
}

const RUN_FAILURES = new Set(['device_replaced', 'device_lost', 'device_protocol', 'chat_unlinked', 'agent_ended']);

/**
 * The text of a failed reply: codes of the server become sentences, an API
 * error body its translated message, anything else stays as it is (the
 * computer reports its own errors in German).
 *
 * @param t - Translation function.
 * @param message - Error message from useChat.
 * @returns Text for the user.
 */
export function streamErrorText(t: TranslateFn, message: string): string {
  if (RUN_FAILURES.has(message)) {
    return t(`agent.failures.${message}` as MessageKey);
  }
  try {
    const body = JSON.parse(message) as { error?: { code?: string; message?: string } };
    if (body.error?.code) {
      return t(`errors.${body.error.code}` as MessageKey, { detail: body.error.message ?? '' });
    }
  } catch {
    // Not JSON: shown as it is.
  }
  return message;
}

function StreamError({ error, onRetry }: { error: Error; onRetry: () => void }) {
  const t = useT();
  return (
    <div role="alert" className="flex items-start gap-3 rounded-xl bg-destructive-subtle p-4 text-sm text-destructive">
      <CircleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="font-medium">{t('chat.streamError')}</p>
        {error.message ? <p className="break-words opacity-90">{streamErrorText(t, error.message)}</p> : null}
      </div>
      <Button variant="outline" size="sm" onClick={onRetry}>
        <RefreshCw />
        {t('common.retry')}
      </Button>
    </div>
  );
}
