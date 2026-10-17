import { ArrowUp, Square } from 'lucide-react';
import { type FormEvent, type KeyboardEvent, type ReactNode, useLayoutEffect, useRef } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Kbd } from '@/components/ui/display.tsx';
import { Tooltip } from '@/components/ui/menu.tsx';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

/**
 * Props of {@link Composer}.
 */
export interface ComposerProps {
  /** Current text. */
  value: string;
  /** Called on every edit. */
  onValueChange: (value: string) => void;
  /** Called with the trimmed text when the user sends. */
  onSubmit: (text: string) => void;
  /** Called when the user stops a running response. */
  onStop?: () => void;
  /** A response is being generated; the send button becomes a stop button. */
  busy?: boolean;
  /** Reason sending is impossible (e.g. no model); shown as tooltip on the send button. */
  blockedReason?: string | null;
  /** Controls rendered in the bottom-left corner, e.g. the model picker. */
  toolbar?: ReactNode;
  /** Focus the input on mount. */
  autoFocus?: boolean;
  /** Placeholder instead of the default one, e.g. for a linked agent. */
  placeholder?: string;
}

/**
 * Chat input: auto-growing textarea, toolbar and send/stop button.
 * Enter sends, Shift+Enter inserts a newline, IME composition is respected.
 *
 * @param props - See {@link ComposerProps}.
 * @returns The composer form.
 */
export function Composer({
  value,
  onValueChange,
  onSubmit,
  onStop,
  busy = false,
  blockedReason,
  toolbar,
  autoFocus,
  placeholder,
}: ComposerProps) {
  const t = useT();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const canSend = value.trim().length > 0 && !blockedReason && !busy;

  // Grow with the content; the max height comes from the max-h class.
  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }
    textarea.style.height = 'auto';
    textarea.style.height = `${textarea.scrollHeight}px`;
  }, [value]);

  function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!canSend) {
      return;
    }
    onSubmit(value.trim());
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      submit();
    }
  }

  const sendButton = (
    <Button
      type="submit"
      variant="primary"
      size="icon-sm"
      className="rounded-full"
      aria-label={t('chat.send')}
      aria-disabled={!canSend || undefined}
      onClick={(event) => {
        if (!canSend) {
          event.preventDefault();
        }
      }}
    >
      <ArrowUp />
    </Button>
  );

  return (
    <form
      onSubmit={submit}
      className={cn(
        'flex flex-col rounded-2xl bg-card shadow-elevation-2 ring-1 ring-border',
        'transition-shadow duration-(--duration-fast) ease-standard focus-within:ring-(length:--focus-ring-width) focus-within:ring-ring/40',
      )}
    >
      <textarea
        ref={textareaRef}
        value={value}
        onChange={(event) => onValueChange(event.target.value)}
        onKeyDown={onKeyDown}
        placeholder={placeholder ?? t('chat.placeholder')}
        aria-label={placeholder ?? t('chat.placeholder')}
        rows={1}
        autoFocus={autoFocus}
        className="max-h-60 min-h-12 w-full resize-none overflow-y-auto bg-transparent px-4 pt-3.5 pb-2 text-base text-foreground outline-none placeholder:text-muted-foreground"
      />
      <div className="flex items-center gap-2 px-2 pb-2">
        <div className="flex min-w-0 flex-1 items-center gap-1">{toolbar}</div>
        {busy ? (
          <Button type="button" variant="secondary" size="icon-sm" className="rounded-full" aria-label={t('chat.stop')} onClick={onStop}>
            <Square className="size-3.5 fill-current" />
          </Button>
        ) : blockedReason ? (
          <Tooltip content={blockedReason}>{sendButton}</Tooltip>
        ) : (
          sendButton
        )}
      </div>
    </form>
  );
}

/**
 * Keyboard hint and disclaimer below the composer; hidden on touch devices.
 *
 * @returns The hint line.
 */
export function ComposerHint() {
  const t = useT();
  return (
    <p className="mt-2 flex flex-wrap items-center justify-center gap-x-3 gap-y-1 text-center text-xs text-muted-foreground pointer-coarse:hidden">
      <span className="inline-flex items-center gap-1">
        <Kbd>{t('chat.keyEnter')}</Kbd>
        {t('chat.hintSend')}
      </span>
      <span className="inline-flex items-center gap-1">
        <Kbd>{t('chat.keyShift')}</Kbd>+<Kbd>{t('chat.keyEnter')}</Kbd>
        {t('chat.hintNewline')}
      </span>
      <span aria-hidden="true">·</span>
      <span>{t('chat.disclaimer')}</span>
    </p>
  );
}
