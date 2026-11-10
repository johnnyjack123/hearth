import type { CalculationResult, CalculatorOutput } from '@hearth/shared';
import type { ToolUIPart } from 'ai';
import { Calculator, CircleAlert } from 'lucide-react';
import type { ReactNode } from 'react';
import { Badge } from '@/components/ui/display.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import { useT } from '@/lib/preferences.tsx';
import { ToolDuration } from './tool-duration.tsx';

/** The stored output, if it has the expected shape. */
function readOutput(value: unknown): CalculatorOutput | null {
  return value && typeof value === 'object' && Array.isArray((value as CalculatorOutput).results) ? (value as CalculatorOutput) : null;
}

/**
 * What the model calculated with the calculator tool: each expression with
 * its result, so the user sees the numbers the answer rests on.
 *
 * @param props - The tool part, whether the reply is still streaming and how
 *   long the call ran.
 * @returns The card.
 */
export function CalculatorCall({ part, streaming, ms }: { part: ToolUIPart; streaming: boolean; ms?: number | undefined }) {
  const t = useT();
  const output = part.state === 'output-available' ? readOutput(part.output) : null;
  const input = part.input as { expressions?: unknown } | undefined;
  // While the call streams in, the expressions show without results.
  const entries: CalculationResult[] =
    output?.results ??
    (Array.isArray(input?.expressions) ? input.expressions.filter((e): e is string => typeof e === 'string').map((expression) => ({ expression })) : []);
  const running = streaming && (part.state === 'input-streaming' || part.state === 'input-available');
  const interrupted = !streaming && (part.state === 'input-streaming' || part.state === 'input-available');

  let status: ReactNode = null;
  if (running) {
    status = (
      <span className="inline-flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
        <Spinner className="size-3.5" />
        {t('calculator.running')}
      </span>
    );
  } else if (part.state === 'output-error') {
    status = (
      <Badge variant="destructive" className="shrink-0">
        <CircleAlert aria-hidden="true" />
        {t('calculator.failed')}
      </Badge>
    );
  } else if (interrupted) {
    status = <Badge className="shrink-0">{t('calculator.interrupted')}</Badge>;
  }

  return (
    <div className="flex min-w-0 flex-col rounded-lg border border-border bg-card text-sm">
      <div className="flex min-w-0 items-center gap-2 px-3 py-2">
        <Calculator className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate text-foreground">{t('calculator.title')}</span>
        <ToolDuration ms={ms} running={running} />
        {status}
      </div>
      {entries.length > 0 ? (
        <ul className="flex flex-col gap-1 border-t border-border px-3 py-2 font-mono text-xs">
          {entries.map((entry, index) => (
            <li key={index} className="flex min-w-0 flex-wrap items-baseline gap-x-2">
              <span className="min-w-0 break-all text-muted-foreground">{entry.expression}</span>
              {entry.error !== undefined ? (
                <span className="min-w-0 break-words text-destructive">{t('calculator.error', { message: entry.error })}</span>
              ) : entry.result !== undefined ? (
                <span className="min-w-0 break-all text-foreground">= {entry.result}</span>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
      {part.state === 'output-error' ? <p className="border-t border-border px-3 py-2 text-muted-foreground">{part.errorText}</p> : null}
    </div>
  );
}
