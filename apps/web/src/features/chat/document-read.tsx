import type { ToolUIPart } from 'ai';
import { CircleAlert, FileText } from 'lucide-react';
import { Spinner } from '@/components/ui/spinner.tsx';
import { useT } from '@/lib/preferences.tsx';

/**
 * One line in a reply when the model read part of a long document: which
 * document and what it looked for, so the user sees where the answer rests.
 *
 * @param props - The tool part and whether the reply is still written.
 * @returns The line.
 */
export function DocumentRead({ part, streaming }: { part: ToolUIPart; streaming: boolean }) {
  const t = useT();
  const input = (part.input ?? {}) as { file?: unknown; focus?: unknown; grep?: unknown; sections?: unknown };
  const output = part.state === 'output-available' ? (part.output as { file?: unknown } | undefined) : undefined;
  const name = typeof output?.file === 'string' ? output.file : typeof input.file === 'string' ? input.file : '…';
  const what =
    typeof input.focus === 'string' && input.focus
      ? input.focus
      : typeof input.grep === 'string' && input.grep
        ? input.grep
        : Array.isArray(input.sections)
          ? input.sections.map((n) => `§${String(n)}`).join(', ')
          : '';
  const running = streaming && (part.state === 'input-streaming' || part.state === 'input-available');
  return (
    <div className="flex min-w-0 flex-col gap-1 text-sm">
      <div className="flex min-w-0 items-center gap-2 text-muted-foreground">
        {running ? <Spinner className="size-4 shrink-0" /> : <FileText className="size-4 shrink-0" aria-hidden="true" />}
        <span className="min-w-0 break-words">
          {what ? t('files.read.part', { name, what }) : t('files.read.whole', { name })}
        </span>
      </div>
      {part.state === 'output-error' ? (
        <p className="ms-6 flex min-w-0 items-start gap-1.5 text-xs text-destructive">
          <CircleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
          <span className="break-words">{part.errorText}</span>
        </p>
      ) : null}
    </div>
  );
}
