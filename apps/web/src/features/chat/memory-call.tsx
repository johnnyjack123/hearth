import { FORGET_TOOL } from '@harness/shared';
import { Link } from '@tanstack/react-router';
import { BookMarked, CircleAlert } from 'lucide-react';
import { Spinner } from '@/components/ui/spinner.tsx';
import { useT } from '@/lib/preferences.tsx';

/** What a call of `remember` or `forget` shows, from a chat model or an agent. */
export interface MemoryCallView {
  /** `remember` or `forget`. */
  tool: string;
  /** The memory's text, once known. */
  text: string | null;
  /** The call runs. */
  running: boolean;
  /** Why it failed, if it did. */
  error: string | null;
}

/**
 * Reads a memory call: the stored output of a chat model's call, or for an
 * agent its input (remember) or the line pi got back (forget).
 *
 * @param tool - Tool name.
 * @param part - The tool part's fields.
 * @param streaming - Whether the reply is still written.
 * @returns The view.
 */
export function memoryCallView(
  tool: string,
  part: { state: string; input?: unknown; output?: unknown; errorText?: string },
  streaming: boolean,
): MemoryCallView {
  const output = part.output as { text?: unknown } | undefined;
  const input = part.input as { text?: unknown } | undefined;
  let text: string | null = null;
  if (part.state === 'output-available' && typeof output?.text === 'string') {
    // An agent's output is the line the model read: "Remembered [id]: …".
    text = output.text.replace(/^(Remembered|Forgot) \[[^\]]*\]: /, '');
  } else if (typeof input?.text === 'string') {
    text = input.text;
  }
  return {
    tool,
    text,
    running: streaming && (part.state === 'input-streaming' || part.state === 'input-available'),
    error: part.state === 'output-error' ? (part.errorText ?? '') : null,
  };
}

/**
 * One line in a reply when a model stored or removed a memory, so the user
 * always sees what every later chat will be told; with a link to the
 * memory in the settings.
 *
 * @param props - See {@link MemoryCallView}.
 * @returns The line.
 */
export function MemoryCall({ tool, text, running, error }: MemoryCallView) {
  const t = useT();
  const forget = tool === FORGET_TOOL;
  return (
    <div className="flex min-w-0 flex-col gap-1 text-sm">
      <div className="flex min-w-0 items-center gap-2 text-muted-foreground">
        {running ? <Spinner className="size-4 shrink-0" /> : <BookMarked className="size-4 shrink-0" aria-hidden="true" />}
        <span className="min-w-0 break-words">
          {error !== null
            ? t(forget ? 'memory.call.forgetFailed' : 'memory.call.rememberFailed')
            : t(forget ? 'memory.call.forgot' : 'memory.call.remembered', { text: text ?? '…' })}
        </span>
        <Link to="/settings" search={{ tab: 'memory' }} className="shrink-0 text-xs text-primary underline-offset-4 hover:underline">
          {t('memory.call.manage')}
        </Link>
      </div>
      {error ? (
        <p className="ms-6 flex min-w-0 items-start gap-1.5 text-xs text-destructive">
          <CircleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span className="break-words">{error}</span>
        </p>
      ) : null}
    </div>
  );
}
