import { type InjectionAssessment, type InjectionFinding, WEB_FETCH_WARNINGS, type WebFetchWarning } from '@harness/shared';
import { ChevronRight, Scale, TriangleAlert } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { type ReactNode, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button.tsx';
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
 *   and how to ask for it.
 * @returns The warning bar.
 */
export function InjectionWarning({
  warnings,
  findings,
  assessment,
  onAssess,
}: {
  warnings: WebFetchWarning[];
  findings: InjectionFinding[] | undefined;
  assessment?: InjectionAssessment | undefined;
  onAssess?: (() => Promise<void>) | undefined;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
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
