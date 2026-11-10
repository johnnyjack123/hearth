import { FORGET_TOOL, MEMORY_PENDING_NOTE, memoryRef } from '@hearth/shared';
import { useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { BookMarked, CircleAlert } from 'lucide-react';
import { useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import { api, errorMessage } from '@/lib/api.ts';
import { useT } from '@/lib/preferences.tsx';
import { queryKeys, useMemories } from '@/lib/queries.ts';

/** What a call of `remember` or `forget` shows, from a chat model or an agent. */
export interface MemoryCallView {
  /** `remember` or `forget`. */
  tool: string;
  /** The memory's text, once known. */
  text: string | null;
  /** The memory's short id, once stored. */
  ref: string | null;
  /** Stored as a suggestion the user has to confirm. */
  suggested: boolean;
  /** The call runs. */
  running: boolean;
  /** Why it failed, if it did. */
  error: string | null;
}

/**
 * Reads a memory call: the stored output of a chat model's call, or for an
 * agent its input (remember) or the line pi got back.
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
  const output = part.output as { text?: unknown; ref?: unknown; pending?: unknown } | undefined;
  const input = part.input as { text?: unknown } | undefined;
  let text: string | null = null;
  let ref: string | null = null;
  let suggested = false;
  if (part.state === 'output-available' && typeof output?.text === 'string') {
    // An agent's output is the line the model read: "Remembered [id]: …".
    const line = /^(?:Remembered|Forgot) \[([^\]]*)\]: /.exec(output.text);
    text = line ? output.text.slice(line[0].length) : output.text;
    ref = typeof output.ref === 'string' ? output.ref : (line?.[1] ?? null);
    suggested = output.pending === true || text.endsWith(MEMORY_PENDING_NOTE);
    if (text.endsWith(MEMORY_PENDING_NOTE)) {
      text = text.slice(0, -MEMORY_PENDING_NOTE.length);
    }
  } else if (typeof input?.text === 'string') {
    text = input.text;
  }
  return {
    tool,
    text,
    ref,
    suggested,
    running: streaming && (part.state === 'input-streaming' || part.state === 'input-available'),
    error: part.state === 'output-error' ? (part.errorText ?? '') : null,
  };
}

/**
 * One line in a reply when a model stored or removed a memory, so the user
 * always sees what every later chat will be told. What a model stores waits
 * for the user: the line offers to confirm or discard it.
 *
 * @param props - See {@link MemoryCallView}.
 * @returns The line.
 */
export function MemoryCall({ tool, text, ref, suggested, running, error }: MemoryCallView) {
  const t = useT();
  const queryClient = useQueryClient();
  const memories = useMemories();
  const [busy, setBusy] = useState(false);
  const forget = tool === FORGET_TOOL;
  const memory = ref && !forget ? memories.data?.find((entry) => memoryRef(entry.id) === ref) : undefined;
  const pending = memory?.pending === true;
  // A suggestion that is gone was discarded (or removed later).
  const gone = suggested && !forget && memories.isSuccess && !memory;

  async function run(action: () => Promise<unknown>) {
    setBusy(true);
    try {
      await action();
      await queryClient.invalidateQueries({ queryKey: queryKeys.memories });
    } catch (failure) {
      toast.error(errorMessage(t, failure));
    } finally {
      setBusy(false);
    }
  }

  let label: string;
  if (error !== null) {
    label = t(forget ? 'memory.call.forgetFailed' : 'memory.call.rememberFailed');
  } else if (forget) {
    label = t('memory.call.forgot', { text: text ?? '…' });
  } else if (gone) {
    label = t('memory.call.discarded', { text: text ?? '…' });
  } else if (pending || (suggested && !memory)) {
    label = t('memory.call.suggested', { text: text ?? '…' });
  } else {
    label = t('memory.call.remembered', { text: text ?? '…' });
  }
  return (
    <div className="flex min-w-0 flex-col gap-1 text-sm">
      <div className="flex min-w-0 flex-wrap items-center gap-2 text-muted-foreground">
        {running ? <Spinner className="size-4 shrink-0" /> : <BookMarked className="size-4 shrink-0" aria-hidden="true" />}
        <span className="min-w-0 break-words">{label}</span>
        {pending && memory ? (
          <span className="flex shrink-0 gap-1">
            <Button variant="secondary" size="sm" loading={busy} onClick={() => void run(() => api.memories.confirm(memory.id))}>
              {t('memory.confirm')}
            </Button>
            <Button variant="ghost" size="sm" disabled={busy} onClick={() => void run(() => api.memories.remove(memory.id))}>
              {t('memory.discard')}
            </Button>
          </span>
        ) : null}
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
