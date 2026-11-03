import { CircleAlert, FileText } from 'lucide-react';
import { Badge } from '@/components/ui/display.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import type { TranslateFn } from '@/i18n/index.ts';
import { useT } from '@/lib/preferences.tsx';
import { type DocumentReadInfo, documentReadInfo } from './document-info.ts';

/** The short label next to the line, and its explanation on hover. */
function readLabel(t: TranslateFn, info: DocumentReadInfo): { badge: string; hint: string } | null {
  switch (info.kind) {
    case 'sections': {
      const badge = t('webFetch.sectionsBadge', { shown: String(info.shown ?? 0), total: String(info.total ?? 0) });
      if (!info.matched) {
        return { badge: `${badge} · ${t('files.read.noMatch')}`, hint: t('files.read.hintNoMatch') };
      }
      if (info.ranking === 'meaning') {
        return { badge: `${badge} · ${t('webFetch.byMeaning')}`, hint: t('webFetch.rankedByMeaning') };
      }
      if (info.ranking === 'keywords') {
        return { badge: `${badge} · ${t('webFetch.byKeywords')}`, hint: t('webFetch.rankedByKeywords') };
      }
      return { badge: `${badge} · ${t('files.read.byNumber')}`, hint: t('files.read.hintNumber') };
    }
    case 'grep':
      return { badge: t('files.read.grepBadge', { count: String(info.lines ?? 0) }), hint: t('files.read.hintGrep') };
    case 'whole':
      return { badge: t('files.read.wholeBadge'), hint: t('files.read.hintWhole') };
    default:
      return null;
  }
}

/**
 * One line in a reply when the model (or an agent) read part of a long
 * document: which document, what it looked for and how it was read
 * (semantic search, keywords, exact words, by number), so the user sees
 * where the answer rests.
 *
 * @param props - The tool part and whether the reply is still written.
 * @returns The line.
 */
export function DocumentRead({ part, streaming }: { part: { state: string; input?: unknown; output?: unknown; errorText?: string }; streaming: boolean }) {
  const t = useT();
  const input = (part.input ?? {}) as { file?: unknown; focus?: unknown; grep?: unknown; sections?: unknown };
  const output = part.state === 'output-available' ? ((part.output ?? {}) as { file?: unknown; text?: unknown }) : null;
  const info = typeof output?.text === 'string' ? documentReadInfo(output.text) : null;
  const name = typeof output?.file === 'string' ? output.file : (info?.name ?? (typeof input.file === 'string' ? input.file : '…'));
  const what =
    typeof input.focus === 'string' && input.focus
      ? input.focus
      : typeof input.grep === 'string' && input.grep
        ? input.grep
        : Array.isArray(input.sections)
          ? input.sections.map((n) => `§${String(n)}`).join(', ')
          : '';
  const label = info ? readLabel(t, info) : null;
  const running = streaming && (part.state === 'input-streaming' || part.state === 'input-available');
  return (
    <div className="flex min-w-0 flex-col gap-1 text-sm">
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-muted-foreground">
        {running ? <Spinner className="size-4 shrink-0" /> : <FileText className="size-4 shrink-0" aria-hidden="true" />}
        <span className="min-w-0 break-words">{what ? t('files.read.part', { name, what }) : t('files.read.whole', { name })}</span>
        {label ? (
          <Badge className="shrink-0" title={label.hint}>
            {label.badge}
          </Badge>
        ) : null}
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
