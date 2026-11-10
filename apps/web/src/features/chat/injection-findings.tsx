import {
  type InjectionAssessment,
  type InjectionFinding,
  type InspectedText,
  type Inspection,
  WEB_FETCH_WARNINGS,
  type WebFetchWarning,
} from '@hearth/shared';
import { ChevronDown, ChevronRight, ChevronUp, ScanSearch, Scale, TriangleAlert } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
import { Skeleton } from '@/components/ui/display.tsx';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog.tsx';
import { SegmentedControl } from '@/components/ui/tabs.tsx';
import { errorMessage } from '@/lib/api.ts';
import { transitions } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';

/**
 * Checks a warning kind read from the stored history.
 *
 * @param value - Stored value.
 * @returns True for a known kind.
 */
export function isWarning(value: unknown): value is WebFetchWarning {
  return WEB_FETCH_WARNINGS.includes(value as WebFetchWarning);
}

/**
 * Checks a finding read from the stored history.
 *
 * @param value - Stored value.
 * @returns True if it has the shape of an {@link InjectionFinding}.
 */
export function isFinding(value: unknown): value is InjectionFinding {
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

/** A piece of found text: plain, an invisible character, or text hidden in tag characters. */
export type RevealedPart = { type: 'text'; text: string } | { type: 'char'; code: string } | { type: 'tags'; text: string };

// Control, zero-width, direction and tag characters. Shown as they are,
// they would hide text or reorder what the user reads.
const HIDDEN =
  /([\u{E0000}-\u{E007F}]+)|([\u{0}-\u{1F}\u{7F}-\u{9F}\u{AD}\u{200B}-\u{200F}\u{202A}-\u{202E}\u{2060}-\u{2064}\u{2066}-\u{2069}\u{FEFF}])/gu;

/**
 * Splits text so invisible characters can be shown: each one as its code
 * point, a run of tag characters as the ASCII text it spells.
 *
 * @param text - Text from a finding.
 * @returns The parts in order.
 */
export function revealHidden(text: string): RevealedPart[] {
  const parts: RevealedPart[] = [];
  let last = 0;
  for (const match of text.matchAll(HIDDEN)) {
    if (match.index > last) {
      parts.push({ type: 'text', text: text.slice(last, match.index) });
    }
    if (match[1]) {
      const hidden = Array.from(match[1], (char) => (char.codePointAt(0) ?? 0) - 0xe0000)
        .filter((code) => code >= 0x20 && code < 0x7f)
        .map((code) => String.fromCharCode(code))
        .join('');
      parts.push({ type: 'tags', text: hidden });
    } else {
      const code = (match[2] ?? '').charCodeAt(0);
      parts.push({ type: 'char', code: `U+${code.toString(16).toUpperCase().padStart(4, '0')}` });
    }
    last = match.index + match[0].length;
  }
  if (last < text.length) {
    parts.push({ type: 'text', text: text.slice(last) });
  }
  return parts;
}

/**
 * Text from the web with invisible characters made visible, see
 * {@link revealHidden}.
 *
 * @param props - The text.
 * @returns The text with marked hidden characters.
 */
export function RevealedText({ text }: { text: string }) {
  const t = useT();
  return revealHidden(text).map((part, index): ReactNode => {
    if (part.type === 'text') {
      return part.text;
    }
    const label = part.type === 'tags' ? t('webFetch.hiddenText', { text: part.text }) : t('webFetch.hiddenChar', { code: part.code });
    return (
      <span key={index} className="mx-0.5 rounded-sm bg-destructive-subtle px-1 font-sans text-destructive">
        {part.type === 'tags' ? label : part.code}
        {part.type === 'char' ? <span className="sr-only">{label}</span> : null}
      </span>
    );
  });
}

/**
 * Where content looked like a prompt injection: per kind its name, a
 * sentence on why it is dangerous, and the places with their surroundings,
 * the match highlighted and invisible characters made visible.
 *
 * @param props - The kinds of warnings, and the findings if they were stored.
 * @returns The list.
 */
export function InjectionFindings({ warnings, findings }: { warnings: WebFetchWarning[]; findings: InjectionFinding[] | undefined }) {
  const t = useT();
  return (
    <div className="flex flex-col gap-3">
      <ul className="flex flex-col gap-3">
        {warnings.map((kind) => (
          <li key={kind} className="flex flex-col gap-1.5">
            <p className="break-words">
              <span className="font-medium">{t(`webFetch.warnings.${kind}`)}</span>
              {' · '}
              {t(`webFetch.why.${kind}`)}
            </p>
            {(findings ?? [])
              .filter((finding) => finding.kind === kind)
              .map((finding, index) => (
                <p
                  key={index}
                  className="wrap-anywhere rounded-md bg-card px-2 py-1.5 font-mono text-xs text-card-foreground"
                >
                  <RevealedText text={finding.before} />
                  <mark className="rounded-sm bg-warning px-0.5 font-medium text-warning-foreground underline decoration-2 underline-offset-2">
                    <RevealedText text={finding.match} />
                  </mark>
                  <RevealedText text={finding.after} />
                </p>
              ))}
          </li>
        ))}
      </ul>
      {findings === undefined ? <p className="text-xs">{t('webFetch.findingsMissing')}</p> : null}
    </div>
  );
}

/**
 * Checks Jev's opinion read from the stored history.
 *
 * @param value - Stored value.
 * @returns True if it has the shape of an {@link InjectionAssessment}.
 */
export function isAssessment(value: unknown): value is InjectionAssessment {
  const assessment = value as Partial<InjectionAssessment> | null;
  return typeof assessment === 'object' && assessment !== null && typeof assessment.model === 'string' && typeof assessment.danger === 'number';
}

/** Words for Jev's danger, from attack to false alarm. */
function dangerLevel(danger: number): 'attack' | 'suspicious' | 'unclear' | 'harmless' {
  if (danger >= 0.75) {
    return 'attack';
  }
  if (danger >= 0.5) {
    return 'suspicious';
  }
  return danger >= 0.25 ? 'unclear' : 'harmless';
}

/**
 * Jev's second opinion on a warning, or the button that asks for it. The
 * warning stays either way: the content may address Jev as well.
 *
 * @param props - The opinion if there is one, and how to ask for it.
 * @returns The row, or nothing without opinion and without a way to ask.
 */
export function AssessmentRow({ assessment, onAssess }: { assessment: InjectionAssessment | undefined; onAssess: (() => Promise<void>) | undefined }) {
  const t = useT();
  const [asking, setAsking] = useState(false);
  if (assessment) {
    const percent = Math.round(Math.min(1, Math.max(0, assessment.danger)) * 100);
    return (
      <p className="ms-6 flex items-start gap-1.5 text-xs" title={t('assessment.model', { model: assessment.model })}>
        <Scale className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
        <span className="min-w-0">
          <span className="font-medium">{t('assessment.result', { percent, level: t(`assessment.levels.${dangerLevel(assessment.danger)}`) })}</span>{' '}
          {t('assessment.note')}
        </span>
      </p>
    );
  }
  if (!onAssess) {
    return null;
  }
  async function ask() {
    setAsking(true);
    try {
      await onAssess?.();
    } catch (error) {
      toast.error(errorMessage(t, error));
    } finally {
      setAsking(false);
    }
  }
  return (
    <Button
      variant="link"
      size="sm"
      loading={asking}
      onClick={() => void ask()}
      className="ms-6 h-auto gap-1 self-start px-0 py-1 text-warning-foreground underline"
    >
      <Scale aria-hidden="true" />
      {asking ? t('assessment.asking') : t('assessment.ask')}
    </Button>
  );
}

/**
 * The warning bar under a web fetch, search or agent tool block: which
 * patterns were found, on demand where and why they matter, and Jev's
 * second opinion.
 *
 * @param props - Kinds of warnings, the findings if stored, Jev's opinion
 *   and how to ask for it, and how to load the whole content (once the
 *   reply is stored).
 * @returns The warning bar.
 */
export function InjectionWarning({
  warnings,
  findings,
  assessment,
  onAssess,
  onInspect,
}: {
  warnings: WebFetchWarning[];
  findings: InjectionFinding[] | undefined;
  assessment?: InjectionAssessment | undefined;
  onAssess?: (() => Promise<void>) | undefined;
  onInspect?: (() => Promise<Inspection>) | undefined;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [inspecting, setInspecting] = useState(false);
  return (
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
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="ms-6 h-auto gap-1 self-start px-0 py-1 text-warning-foreground underline"
      >
        <ChevronRight
          className={cn('size-3.5 transition-transform duration-(--duration-fast) ease-standard', open && 'rotate-90')}
          aria-hidden="true"
        />
        {open ? t('webFetch.hideFindings') : t('webFetch.showFindings')}
      </Button>
      {onInspect ? (
        <>
          <Button
            variant="link"
            size="sm"
            onClick={() => setInspecting(true)}
            className="ms-6 h-auto gap-1 self-start px-0 py-1 text-warning-foreground underline"
          >
            <ScanSearch className="size-3.5" aria-hidden="true" />
            {t('inspection.open')}
          </Button>
          <InspectionDialog open={inspecting} onOpenChange={setInspecting} load={onInspect} />
        </>
      ) : null}
      <AnimatePresence initial={false}>
        {open ? (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1, transition: transitions.enter }}
            exit={{ opacity: 0, transition: transitions.exit }}
            className="ms-6 pb-1"
          >
            <InjectionFindings warnings={warnings} findings={findings} />
          </motion.div>
        ) : null}
      </AnimatePresence>
      <AssessmentRow assessment={assessment} onAssess={onAssess} />
    </div>
  );
}

/** A run of the text: plain, or a marked place with the patterns that matched it. */
type Segment = { text: string; place: null } | { text: string; place: { index: number; kinds: WebFetchWarning[] } };

/**
 * Cuts a text at its marks. Overlapping marks (a comment that also
 * addresses the AI) become one place, so every place is counted once.
 *
 * @param inspected - The text with its marks.
 * @returns The runs in order and the number of places.
 */
export function segments(inspected: InspectedText): { runs: Segment[]; places: number } {
  const runs: Segment[] = [];
  const merged: { start: number; end: number; kinds: WebFetchWarning[] }[] = [];
  for (const mark of [...inspected.marks].sort((a, b) => a.start - b.start)) {
    const last = merged.at(-1);
    if (last && mark.start < last.end) {
      last.end = Math.max(last.end, mark.end);
      if (!last.kinds.includes(mark.kind)) {
        last.kinds.push(mark.kind);
      }
    } else {
      merged.push({ start: mark.start, end: mark.end, kinds: [mark.kind] });
    }
  }
  let at = 0;
  merged.forEach((place, index) => {
    if (place.start > at) {
      runs.push({ text: inspected.text.slice(at, place.start), place: null });
    }
    runs.push({ text: inspected.text.slice(place.start, place.end), place: { index, kinds: place.kinds } });
    at = place.end;
  });
  if (at < inspected.text.length) {
    runs.push({ text: inspected.text.slice(at), place: null });
  }
  return { runs, places: merged.length };
}

function prefersReducedMotion(): boolean {
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * The whole content behind a prompt injection warning: the page, its HTML
 * source, the search hits or an agent's tool output, every flagged place
 * marked. It opens at the first place; the others can be stepped through.
 *
 * @param props - Open state and how to load the content.
 * @returns The dialog.
 */
export function InspectionDialog({
  open,
  onOpenChange,
  load,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  load: () => Promise<Inspection>;
}) {
  const t = useT();
  const [state, setState] = useState<{ inspection: Inspection } | { error: string } | null>(null);
  const [textIndex, setTextIndex] = useState(0);
  const [current, setCurrent] = useState(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const moved = useRef(false);

  useEffect(() => {
    if (!open) {
      return;
    }
    let cancelled = false;
    setState(null);
    load().then(
      (inspection) => {
        if (cancelled) {
          return;
        }
        // Start where the first place is; hidden text may be in the source only.
        const first = inspection.texts.findIndex((text) => text.marks.length > 0);
        setTextIndex(Math.max(0, first));
        setCurrent(0);
        moved.current = false;
        setState({ inspection });
      },
      (error: unknown) => {
        if (!cancelled) {
          setState({ error: errorMessage(t, error) });
        }
      },
    );
    return () => {
      cancelled = true;
    };
    // Loaded once per opening; `t` and `load` change with every render.
  }, [open]);

  const inspection = state && 'inspection' in state ? state.inspection : null;
  const shown = inspection?.texts[textIndex];
  const { runs, places } = shown ? segments(shown) : { runs: [], places: 0 };

  useEffect(() => {
    const target = scrollRef.current?.querySelector(`[data-place="${current}"]`);
    if (!target) {
      return;
    }
    // The first jump is instant, stepping glides unless motion is reduced.
    target.scrollIntoView({ block: 'center', behavior: moved.current && !prefersReducedMotion() ? 'smooth' : 'auto' });
    moved.current = true;
  }, [current, textIndex, inspection]);

  function step(by: number) {
    if (places > 0) {
      setCurrent((index) => (index + by + places) % places);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex h-[min(85vh,calc(var(--spacing)*220))] max-w-4xl flex-col gap-3">
        <DialogHeader>
          <DialogTitle>{t('inspection.title')}</DialogTitle>
          <DialogDescription className="break-all font-mono text-xs">{inspection?.location ?? ' '}</DialogDescription>
        </DialogHeader>
        {state === null ? (
          <div className="flex flex-col gap-2" aria-busy="true" aria-label={t('common.loading')}>
            <Skeleton className="h-8 w-1/2" />
            <Skeleton className="h-64 w-full" />
          </div>
        ) : 'error' in state ? (
          <p className="flex items-start gap-2 text-sm text-destructive">
            <TriangleAlert className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
            {state.error}
          </p>
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-2">
              {inspection && inspection.texts.length > 1 ? (
                <SegmentedControl
                  label={t('inspection.texts')}
                  value={String(textIndex)}
                  onValueChange={(value) => {
                    setTextIndex(Number(value));
                    setCurrent(0);
                    moved.current = false;
                  }}
                  options={inspection.texts.map((text, index) => ({
                    value: String(index),
                    label: t('inspection.textLabel', { kind: t(`inspection.kinds.${text.kind}`), count: segments(text).places }),
                  }))}
                />
              ) : null}
              <span className="flex-1 text-sm text-muted-foreground" aria-live="polite">
                {places > 0 ? t('inspection.position', { index: current + 1, count: places }) : t('inspection.none')}
              </span>
              <Button variant="outline" size="icon-sm" aria-label={t('inspection.previous')} disabled={places < 2} onClick={() => step(-1)}>
                <ChevronUp />
              </Button>
              <Button variant="outline" size="icon-sm" aria-label={t('inspection.next')} disabled={places < 2} onClick={() => step(1)}>
                <ChevronDown />
              </Button>
            </div>
            {inspection && !inspection.complete ? <p className="text-xs text-muted-foreground">{t('inspection.incomplete')}</p> : null}
            {shown?.kind === 'source' ? <p className="text-xs text-muted-foreground">{t('inspection.sourceHint')}</p> : null}
            {shown?.truncated ? <p className="text-xs text-muted-foreground">{t('inspection.truncated')}</p> : null}
            <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto rounded-lg bg-muted p-3" tabIndex={0} aria-label={t('inspection.title')}>
              <pre className="whitespace-pre-wrap wrap-anywhere font-mono text-xs text-foreground">
                {runs.map((run, index) =>
                  run.place ? (
                    <mark
                      key={index}
                      data-place={run.place.index}
                      title={run.place.kinds.map((kind) => t(`webFetch.warnings.${kind}`)).join(', ')}
                      className={cn(
                        'rounded-sm bg-warning px-0.5 text-warning-foreground underline decoration-2 underline-offset-2',
                        run.place.index === current && 'ring-(length:--focus-ring-width) ring-ring',
                      )}
                    >
                      <RevealedText text={run.text} />
                    </mark>
                  ) : (
                    run.text
                  ),
                )}
              </pre>
            </div>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
