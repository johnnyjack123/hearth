import {
  type InjectionFinding,
  parseWebFetchError,
  WEB_FETCH_WARNINGS,
  type WebFetchOutput,
  type WebFetchWarning,
} from '@harness/shared';
import type { ToolUIPart } from 'ai';
import { Check, ChevronRight, CircleAlert, Globe, Info, TriangleAlert } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { type ReactNode, useState } from 'react';
import { Button } from '@/components/ui/button.tsx';
import { Badge } from '@/components/ui/display.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import type { TranslateFn } from '@/i18n/index.ts';
import { transitions } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';
import { ToolText } from './agent-parts.tsx';
import { InjectionFindings } from './injection-findings.tsx';

function isWarning(value: unknown): value is WebFetchWarning {
  return WEB_FETCH_WARNINGS.includes(value as WebFetchWarning);
}

function isFinding(value: unknown): value is InjectionFinding {
  const finding = value as Partial<InjectionFinding> | null;
  return (
    typeof finding === 'object' &&
    finding !== null &&
    isWarning(finding.kind) &&
    typeof finding.before === 'string' &&
    typeof finding.match === 'string' &&
    typeof finding.after === 'string'
  );
}

/** Reads the stored output defensively: it comes from the server's history. */
function readOutput(output: unknown): WebFetchOutput | null {
  if (!output || typeof output !== 'object' || typeof (output as WebFetchOutput).text !== 'string') {
    return null;
  }
  const value = output as WebFetchOutput;
  const { findings, ...rest } = value;
  return {
    ...rest,
    warnings: Array.isArray(value.warnings) ? value.warnings.filter(isWarning) : [],
    // Replies stored before findings existed have none; the block then says so.
    ...(Array.isArray(findings) ? { findings: findings.filter(isFinding) } : {}),
  };
}

function requestedUrl(input: unknown): string {
  const url = input && typeof input === 'object' ? (input as { url?: unknown }).url : undefined;
  return typeof url === 'string' ? url : '';
}

function errorText(t: TranslateFn, raw: string): { code: string | null; text: string } {
  const parsed = parseWebFetchError(raw);
  if (!parsed) {
    return { code: null, text: raw };
  }
  return { code: parsed.code, text: t(`webFetch.errors.${parsed.code}`, { detail: parsed.detail ?? '' }) };
}

/**
 * A web page the model loaded in an ordinary chat: the URL, its state and,
 * on demand, the text the model read. Injection warnings stay visible while
 * the block is closed.
 *
 * @param props - The `tool-web_fetch` part and whether its message is still streaming.
 * @returns The web fetch block.
 */
export function WebFetchCall({ part, streaming }: { part: ToolUIPart; streaming: boolean }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [showFindings, setShowFindings] = useState(false);
  const output = part.state === 'output-available' ? readOutput(part.output) : null;
  const failure = part.state === 'output-error' ? errorText(t, part.errorText) : null;
  const running = streaming && (part.state === 'input-streaming' || part.state === 'input-available');
  // A call without result after the reply ended was cut off by stop.
  const interrupted = !streaming && (part.state === 'input-streaming' || part.state === 'input-available');
  const url = output?.url ?? requestedUrl(part.input);
  const declined = failure?.code === 'denied' || failure?.code === 'no_answer';
  const warnings = output?.warnings ?? [];

  let status: ReactNode;
  if (running) {
    status = (
      <span className="inline-flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
        <Spinner className="size-3.5" />
        {t('webFetch.loading')}
      </span>
    );
  } else if (failure) {
    status = (
      <Badge variant={declined ? 'neutral' : 'destructive'} className="shrink-0">
        <CircleAlert aria-hidden="true" />
        {declined ? t('webFetch.denied') : t('webFetch.failed')}
      </Badge>
    );
  } else if (interrupted) {
    status = <Badge className="shrink-0">{t('webFetch.interrupted')}</Badge>;
  } else {
    status = <Check className="size-4 shrink-0 text-muted-foreground" aria-label={t('webFetch.done')} />;
  }

  return (
    <div className="flex min-w-0 flex-col rounded-lg border border-border bg-card text-sm">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        aria-label={open ? t('webFetch.hideContent') : t('webFetch.showContent')}
        className="flex min-w-0 items-center gap-2 rounded-lg px-3 py-2 text-start transition-colors duration-(--duration-instant) ease-standard hover:bg-accent focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
      >
        <Globe className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground">{output?.title ? `${output.title} · ${url}` : url}</span>
        {status}
        <ChevronRight
          className={cn('size-3.5 shrink-0 text-muted-foreground transition-transform duration-(--duration-fast) ease-standard', open && 'rotate-90')}
          aria-hidden="true"
        />
      </button>
      {warnings.length > 0 ? (
        <div role="note" className="flex flex-col gap-1 border-t border-border bg-warning px-3 py-2 text-warning-foreground last:rounded-b-lg">
          <div className="flex items-start gap-2">
            <TriangleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
            <span className="min-w-0 break-words">
              {t('webFetch.injection', { found: warnings.map((warning) => t(`webFetch.warnings.${warning}`)).join(', ') })}
            </span>
          </div>
          <Button
            variant="link"
            size="sm"
            aria-expanded={showFindings}
            onClick={() => setShowFindings(!showFindings)}
            className="ms-6 h-auto gap-1 self-start px-0 py-1 text-warning-foreground underline"
          >
            <ChevronRight
              className={cn('size-3.5 transition-transform duration-(--duration-fast) ease-standard', showFindings && 'rotate-90')}
              aria-hidden="true"
            />
            {showFindings ? t('webFetch.hideFindings') : t('webFetch.showFindings')}
          </Button>
          <AnimatePresence initial={false}>
            {showFindings ? (
              <motion.div
                initial={{ opacity: 0 }}
                animate={{ opacity: 1, transition: transitions.enter }}
                exit={{ opacity: 0, transition: transitions.exit }}
                className="ms-6 pb-1"
              >
                <InjectionFindings warnings={warnings} findings={output?.findings} />
              </motion.div>
            ) : null}
          </AnimatePresence>
        </div>
      ) : null}
      {failure && !open ? <p className="border-t border-border px-3 py-2 text-muted-foreground">{failure.text}</p> : null}
      <AnimatePresence initial={false}>
        {open ? (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1, transition: transitions.enter }}
            exit={{ opacity: 0, transition: transitions.exit }}
            className="flex flex-col gap-2 border-t border-border px-3 py-2"
          >
            <ToolText label={t('webFetch.source')} text={url} />
            {failure ? <ToolText label={t('webFetch.failed')} text={failure.text} /> : null}
            {output ? <ToolText label={t('webFetch.content')} text={output.text || t('webFetch.noContent')} /> : null}
            {output?.mainContent ? (
              <p className="flex items-start gap-2 text-xs text-muted-foreground">
                <Info className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
                {t('webFetch.mainContent')}
              </p>
            ) : null}
            {output?.truncated ? (
              <p className="flex items-start gap-2 text-xs text-muted-foreground">
                <Info className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
                {t('webFetch.truncated')}
              </p>
            ) : null}
          </motion.div>
        ) : null}
      </AnimatePresence>
    </div>
  );
}
