import { parseWebSearchError, WEB_FETCH_TOOL, type WebSearchOutput, type WebSearchResult } from '@harness/shared';
import type { ToolUIPart } from 'ai';
import { ChevronRight, CircleAlert, Search } from 'lucide-react';
import { AnimatePresence, motion } from 'motion/react';
import { type ReactNode, useState } from 'react';
import { Badge } from '@/components/ui/display.tsx';
import { Spinner } from '@/components/ui/spinner.tsx';
import type { ChatMessage } from '@/lib/api.ts';
import { transitions } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { cn } from '@/lib/utils.ts';
import { InjectionWarning, isFinding, isWarning, RevealedText } from './injection-findings.tsx';

function isResult(value: unknown): value is WebSearchResult {
  const result = value as Partial<WebSearchResult> | null;
  return (
    typeof result === 'object' &&
    result !== null &&
    typeof result.title === 'string' &&
    typeof result.url === 'string' &&
    /^https?:\/\//i.test(result.url) &&
    typeof result.snippet === 'string'
  );
}

/** Reads the stored output defensively: it comes from the server's history. */
function readOutput(output: unknown): WebSearchOutput | null {
  const value = output as Partial<WebSearchOutput> | null;
  if (!value || typeof value !== 'object' || !Array.isArray(value.results)) {
    return null;
  }
  return {
    query: typeof value.query === 'string' ? value.query : '',
    provider: value.provider ?? 'searxng',
    results: value.results.filter(isResult),
    warnings: Array.isArray(value.warnings) ? value.warnings.filter(isWarning) : [],
    findings: Array.isArray(value.findings) ? value.findings.filter(isFinding) : [],
  };
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '');
  } catch {
    return url;
  }
}

/**
 * A link to a page found on the web. It shows the host next to the title,
 * because the title comes from the page and can claim anything.
 *
 * @param props - Address, title and optional extra line.
 * @returns The link.
 */
function ExternalLink({ url, title, children }: { url: string; title: string; children?: ReactNode }) {
  return (
    <span className="flex min-w-0 flex-col gap-0.5">
      <a
        href={url}
        target="_blank"
        rel="noopener noreferrer nofollow"
        className="w-fit max-w-full break-words font-medium text-foreground underline-offset-2 hover:underline focus-visible:rounded-sm focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
      >
        <RevealedText text={title} />
      </a>
      <span className="truncate text-xs text-muted-foreground">
        {hostOf(url)}
        {children}
      </span>
    </span>
  );
}

/**
 * A web search of the model in research mode: the query, how many hits
 * came back and, on demand, the numbered hits with links. Injection
 * warnings stay visible while the block is closed.
 *
 * @param props - The `tool-web_search` part and whether its message is still streaming.
 * @returns The search block.
 */
export function WebSearchCall({ part, streaming }: { part: ToolUIPart; streaming: boolean }) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const output = part.state === 'output-available' ? readOutput(part.output) : null;
  const failure = part.state === 'output-error' ? parseWebSearchError(part.errorText) : null;
  const failureText =
    part.state === 'output-error' ? (failure ? t(`webSearch.errors.${failure.code}`, { detail: failure.detail ?? '' }) : part.errorText) : null;
  const running = streaming && (part.state === 'input-streaming' || part.state === 'input-available');
  const interrupted = !streaming && (part.state === 'input-streaming' || part.state === 'input-available');
  const input = part.input as { query?: unknown } | undefined;
  const query = output?.query || (typeof input?.query === 'string' ? input.query : '');

  let status: ReactNode;
  if (running) {
    status = (
      <span className="inline-flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
        <Spinner className="size-3.5" />
        {t('webSearch.searching')}
      </span>
    );
  } else if (failureText !== null) {
    status = (
      <Badge variant="destructive" className="shrink-0">
        <CircleAlert aria-hidden="true" />
        {t('webSearch.failed')}
      </Badge>
    );
  } else if (interrupted) {
    status = <Badge className="shrink-0">{t('webSearch.interrupted')}</Badge>;
  } else {
    status = (
      <span className="shrink-0 text-xs text-muted-foreground">
        {output && output.results.length > 0 ? t('webSearch.count', { count: output.results.length }) : t('webSearch.noResults')}
      </span>
    );
  }

  return (
    <div className="flex min-w-0 flex-col rounded-lg border border-border bg-card text-sm">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
        aria-label={open ? t('webSearch.hideResults') : t('webSearch.showResults')}
        className="flex min-w-0 items-center gap-2 rounded-lg px-3 py-2 text-start transition-colors duration-(--duration-instant) ease-standard hover:bg-accent focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
      >
        <Search className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
        <span className="min-w-0 flex-1 truncate text-foreground">{t('webSearch.title', { query })}</span>
        {status}
        <ChevronRight
          className={cn('size-3.5 shrink-0 text-muted-foreground transition-transform duration-(--duration-fast) ease-standard', open && 'rotate-90')}
          aria-hidden="true"
        />
      </button>
      {output && output.warnings.length > 0 ? <InjectionWarning warnings={output.warnings} findings={output.findings} /> : null}
      {failureText !== null ? <p className="border-t border-border px-3 py-2 text-muted-foreground">{failureText}</p> : null}
      <AnimatePresence initial={false}>
        {open && output && output.results.length > 0 ? (
          <motion.ol
            initial={{ opacity: 0 }}
            animate={{ opacity: 1, transition: transitions.enter }}
            exit={{ opacity: 0, transition: transitions.exit }}
            className="flex flex-col gap-3 border-t border-border px-3 py-3"
          >
            {output.results.map((result, index) => (
              <li key={result.url} className="flex min-w-0 gap-2">
                <span className="w-5 shrink-0 text-end text-xs leading-5 text-muted-foreground tabular-nums">{index + 1}.</span>
                <div className="flex min-w-0 flex-col gap-1">
                  <ExternalLink url={result.url} title={result.title}>
                    {result.published ? ` · ${result.published}` : null}
                  </ExternalLink>
                  {result.snippet ? (
                    <p className="line-clamp-3 break-words text-muted-foreground">
                      <RevealedText text={result.snippet} />
                    </p>
                  ) : null}
                </div>
              </li>
            ))}
          </motion.ol>
        ) : null}
      </AnimatePresence>
    </div>
  );
}

/**
 * The pages the model read for a reply, numbered, below its text. Only
 * pages that loaded count; search hits alone are no sources.
 *
 * @param props - The assistant message.
 * @returns The list, or nothing without loaded pages.
 */
export function Sources({ message }: { message: ChatMessage }) {
  const t = useT();
  const pages = new Map<string, string>();
  for (const part of message.parts) {
    if (part.type !== `tool-${WEB_FETCH_TOOL}` || !('state' in part) || part.state !== 'output-available') {
      continue;
    }
    const output = part.output as { url?: unknown; title?: unknown } | undefined;
    if (typeof output?.url === 'string' && /^https?:\/\//i.test(output.url) && !pages.has(output.url)) {
      pages.set(output.url, typeof output.title === 'string' && output.title ? output.title : output.url);
    }
  }
  if (pages.size === 0) {
    return null;
  }
  return (
    <section aria-label={t('webSearch.sources')} className="flex flex-col gap-2 border-t border-border pt-3 text-sm">
      <h3 className="text-xs font-medium text-muted-foreground">{t('webSearch.sources')}</h3>
      <ol className="flex flex-col gap-2">
        {[...pages].map(([url, title], index) => (
          <li key={url} className="flex min-w-0 gap-2">
            <span className="w-5 shrink-0 text-end text-xs leading-5 text-muted-foreground tabular-nums">{index + 1}.</span>
            <ExternalLink url={url} title={title} />
          </li>
        ))}
      </ol>
    </section>
  );
}
