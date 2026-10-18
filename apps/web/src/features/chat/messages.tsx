import type { ApprovalDecision } from '@harness/shared';
import { Brain, Check, ChevronRight, CircleAlert, Container, Copy, RefreshCw } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { lazy, Suspense, useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Tooltip } from '@/components/ui/menu.tsx';
import type { MessageKey, TranslateFn } from '@/i18n/index.ts';
import type { ChatMessage } from '@/lib/api.ts';
import { riseIn, transitions } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';
import { AgentNotice, AgentProgress, ApprovalCard, ToolCall } from './agent-parts.tsx';

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
}

/**
 * All messages of a conversation plus the typing indicator and error state.
 *
 * @param props - See {@link MessageListProps}.
 * @returns The message column.
 */
export function MessageList({ messages, status, error, initialIds, onRegenerate, onApprove }: MessageListProps) {
  const lastAssistantId = messages.findLast((message) => message.role === 'assistant')?.id;
  const busy = status === 'submitted' || status === 'streaming';

  return (
    <div className="mx-auto flex w-full max-w-(--content-width) flex-col gap-8 px-4 py-8" aria-live="polite" aria-busy={busy}>
      {messages.map((message, index) => {
        const animate = !initialIds.has(message.id);
        if (message.role === 'user') {
          return <UserMessage key={message.id} message={message} animate={animate} />;
        }
        if (message.role === 'assistant') {
          const isLast = index === messages.length - 1;
          return (
            <AssistantMessage
              key={message.id}
              message={message}
              animate={animate}
              streaming={isLast && status === 'streaming'}
              canRegenerate={message.id === lastAssistantId && !busy}
              onRegenerate={onRegenerate}
              onApprove={onApprove}
            />
          );
        }
        return null;
      })}
      <AnimatePresence>{status === 'submitted' ? <TypingIndicator key="typing" /> : null}</AnimatePresence>
      {error && !busy ? <StreamError error={error} onRetry={onRegenerate} /> : null}
    </div>
  );
}

function UserMessage({ message, animate }: { message: ChatMessage; animate: boolean }) {
  return (
    <motion.div
      variants={riseIn}
      initial={animate ? 'initial' : false}
      animate="animate"
      className="ms-auto max-w-4/5 whitespace-pre-wrap break-words rounded-2xl rounded-ee-sm bg-bubble px-4 py-2.5 text-base leading-7 text-bubble-foreground"
    >
      {messageText(message)}
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
}: {
  message: ChatMessage;
  animate: boolean;
  streaming: boolean;
  canRegenerate: boolean;
  onRegenerate: () => void;
  onApprove: ((id: number, decision: ApprovalDecision) => Promise<void>) | undefined;
}) {
  return (
    <motion.div variants={riseIn} initial={animate ? 'initial' : false} animate="animate" className="group flex min-w-0 flex-col gap-3">
      {message.parts.map((part, index) => {
        if (part.type === 'reasoning' && part.text.trim()) {
          return <Reasoning key={index} text={part.text} streaming={streaming && part.state === 'streaming'} />;
        }
        if (part.type === 'dynamic-tool') {
          return <ToolCall key={part.toolCallId} part={part} streaming={streaming} />;
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
        if (part.type === 'text') {
          return (
            <Suspense key={index} fallback={<p className="whitespace-pre-wrap text-base leading-7">{part.text}</p>}>
              <Markdown animating={streaming}>{part.text}</Markdown>
            </Suspense>
          );
        }
        return null;
      })}
      {!streaming ? <MessageActions message={message} canRegenerate={canRegenerate} onRegenerate={onRegenerate} /> : null}
    </motion.div>
  );
}

function Reasoning({ text, streaming }: { text: string; streaming: boolean }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const expanded = open || streaming;
  return (
    <div className="flex flex-col gap-2">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
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
  const t = useT();
  const [copied, setCopied] = useState(false);
  const usage = message.metadata?.usage;

  async function copy() {
    await navigator.clipboard.writeText(messageText(message));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }

  return (
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
        </span>
      ) : null}
    </div>
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
