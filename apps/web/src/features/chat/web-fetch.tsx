import { type Inspection, parseWebFetchError, type WebFetchGrep, type WebFetchOutput, type WebFetchSections } from '@hearth/shared';
import type { ToolUIPart } from 'ai';
import { Check, ChevronRight, CircleAlert, Globe, Info } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { type ReactNode, useState } from 'react';
import { Badge } from '@/components/ui/display.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import type { TranslateFn } from '@/i18n/index.ts';
import { transitions } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';
import { ToolText } from './agent-parts.tsx';
import { InjectionWarning, isAssessment, isFinding, isWarning } from './injection-findings.tsx';
import { ToolDuration } from './tool-duration.tsx';

/** Reads the stored output defensively: it comes from the server's history. */
function readOutput(output: unknown): WebFetchOutput | null {
  if (!output || typeof output !== 'object' || typeof (output as WebFetchOutput).text !== 'string') {
    return null;
  }
  const value = output as WebFetchOutput;
  const { findings, focus, sections, grep, assessment, ...rest } = value;
  return {
    ...rest,
    ...(isAssessment(assessment) ? { assessment } : {}),
    warnings: Array.isArray(value.warnings) ? value.warnings.filter(isWarning) : [],
    // Replies stored before findings existed have none; the block then says so.
    ...(Array.isArray(findings) ? { findings: findings.filter(isFinding) } : {}),
    ...(typeof focus === 'string' ? { focus } : {}),
    ...(isSections(sections) ? { sections } : {}),
    ...(isGrep(grep) ? { grep } : {}),
  };
}

function isGrep(value: unknown): value is WebFetchGrep {
  const grep = value as WebFetchGrep | undefined;
  return typeof grep?.pattern === 'string' && (grep.lines === null || typeof grep.lines === 'number') && typeof grep.truncated === 'boolean';
}

function isSections(value: unknown): value is WebFetchSections {
  const sections = value as WebFetchSections | undefined;
  return (
    typeof sections?.total === 'number' &&
    Array.isArray(sections.shown) &&
    sections.shown.every((index) => typeof index === 'number') &&
    typeof sections.matched === 'boolean'
  );
}

/** Which part of the page the model got, and why. */
function sectionsNote(t: TranslateFn, output: WebFetchOutput): string {
  const { sections, focus, grep } = output;
  if (grep && grep.lines !== null) {
    return grep.lines === 0
      ? t('webFetch.grepNone', { pattern: grep.pattern })
      : t(grep.truncated ? 'webFetch.grepCut' : 'webFetch.grepShown', { pattern: grep.pattern, count: String(grep.lines) });
  }
  if (!sections) {
    return focus || grep ? t('webFetch.shortPage') : t('webFetch.wholePage');
  }
  const values = { list: sections.shown.join(', '), total: String(sections.total) };
  if (!focus) {
    return t('webFetch.sectionsAsked', values);
  }
  // Replies stored before 23.10.2026 do not say how the sections were picked.
  const how = sections.ranking === 'meaning' ? ` ${t('webFetch.rankedByMeaning')}` : sections.ranking === 'keywords' ? ` ${t('webFetch.rankedByKeywords')}` : '';
  return (sections.matched ? t('webFetch.sectionsChosen', values) : t('webFetch.sectionsNoMatch', values)) + how;
}

/** Short form of {@link sectionsNote} for the closed card; null when nothing was left out. */
function readBadge(t: TranslateFn, output: WebFetchOutput): string | null {
  const { sections, focus, grep } = output;
  if (grep && grep.lines !== null) {
    return t('webFetch.grepBadge', { count: String(grep.lines) });
  }
  if (sections) {
    const badge = t('webFetch.sectionsBadge', { shown: String(sections.shown.length), total: String(sections.total) });
    return sections.ranking ? `${badge} · ${t(sections.ranking === 'meaning' ? 'webFetch.byMeaning' : 'webFetch.byKeywords')}` : badge;
  }
  return focus || grep ? null : t('webFetch.wholeBadge');
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
export function WebFetchCall({
  part,
  streaming,
  ms,
  onAssess,
  onInspect,
}: {
  part: ToolUIPart;
  streaming: boolean;
  ms?: number | undefined;
  onAssess?: (() => Promise<void>) | undefined;
  onInspect?: (() => Promise<Inspection>) | undefined;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const output = part.state === 'output-available' ? readOutput(part.output) : null;
  const failure = part.state === 'output-error' ? errorText(t, part.errorText) : null;
  const running = streaming && (part.state === 'input-streaming' || part.state === 'input-available');
  // A call without result after the reply ended was cut off by stop.
  const interrupted = !streaming && (part.state === 'input-streaming' || part.state === 'input-available');
  const url = output?.url ?? requestedUrl(part.input);
  const declined = failure?.code === 'denied' || failure?.code === 'no_answer';
  const warnings = output?.warnings ?? [];
  const note = output ? sectionsNote(t, output) : null;
  const badge = output ? readBadge(t, output) : null;

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
        {badge ? <Badge className="shrink-0">{badge}</Badge> : null}
        <ToolDuration ms={ms} running={running} />
        {status}
        <ChevronRight
          className={cn('size-3.5 shrink-0 text-muted-foreground transition-transform duration-(--duration-fast) ease-standard', open && 'rotate-90')}
          aria-hidden="true"
        />
      </button>
      {warnings.length > 0 ? (
        <InjectionWarning warnings={warnings} findings={output?.findings} assessment={output?.assessment} onAssess={onAssess} onInspect={onInspect} />
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
            {output?.focus ? <ToolText label={t('webFetch.focus')} text={output.focus} /> : null}
            {output?.grep ? <ToolText label={t('webFetch.grepLabel')} text={output.grep.pattern} /> : null}
            {failure ? <ToolText label={t('webFetch.failed')} text={failure.text} /> : null}
            {output ? <ToolText label={t('webFetch.content')} text={output.text || t('webFetch.noContent')} /> : null}
            {note ? (
              <p className="flex items-start gap-2 text-xs text-muted-foreground">
                <Info className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
                {note}
              </p>
            ) : null}
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
