import { CircleAlert } from 'lucide-react';
import { createContext, type PointerEvent, useContext, useEffect, useRef, useState } from 'react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/menu.tsx';
import { durations } from '@/lib/motion.ts';
import { useT } from '@/lib/preferences.tsx';
import { type KnownSource, pageKey } from './citations.ts';
import { RevealedText } from './injection-findings.tsx';

/** The pages the chat read or found, for the badges inside its replies. */
export const SourcesContext = createContext<ReadonlyMap<string, KnownSource>>(new Map());

/** Waits before opening on hover, so passing the mouse over text opens nothing. */
const OPEN_DELAY_MS = durations.normal * 1000;
/** Lets the pointer travel from the badge to the panel without closing it. */
const CLOSE_DELAY_MS = durations.slow * 1000;

function hostOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, '');
  } catch {
    return url;
  }
}

function readUrls(value: unknown): string[] {
  try {
    const urls: unknown = JSON.parse(typeof value === 'string' ? value : '[]');
    return Array.isArray(urls) ? urls.filter((url): url is string => typeof url === 'string' && /^https?:\/\//i.test(url)) : [];
  } catch {
    return [];
  }
}

/**
 * The sources of one paragraph of a reply, as a small badge at its end: the
 * host of the first source and how many more there are. Pointing at it or
 * clicking it opens a list of all of them with title and host; links the
 * chat never read or found carry a warning. Rendered for the elements that
 * `rehypeSourceBadges` adds.
 *
 * @param props - `data-urls`: the addresses as a JSON array.
 * @returns The badge, or nothing without addresses.
 */
export function SourceBadge(props: Record<string, unknown>) {
  const t = useT();
  const known = useContext(SourcesContext);
  const [open, setOpen] = useState(false);
  const timer = useRef<number | undefined>(undefined);
  // Opened by a click or key: stays until closed the same way, whatever the mouse does.
  const pinned = useRef(false);
  useEffect(() => () => window.clearTimeout(timer.current), []);

  const entries = readUrls(props['data-urls']).map((url) => {
    const key = pageKey(url);
    const source = key ? known.get(key) : undefined;
    return { url: source?.url ?? url, title: source?.title ?? url, read: source?.read ?? false, known: source !== undefined };
  });
  const first = entries[0];
  if (!first) {
    return null;
  }
  const hosts = [...new Set(entries.map((entry) => hostOf(entry.url)))];
  /** Opens or closes after a delay, for a mouse only: touch and keyboard click. */
  const hover = (next: boolean) => (event: PointerEvent) => {
    if (event.pointerType !== 'mouse' || pinned.current) {
      return;
    }
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setOpen(next), next ? OPEN_DELAY_MS : CLOSE_DELAY_MS);
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        window.clearTimeout(timer.current);
        pinned.current = next;
        setOpen(next);
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          onPointerEnter={hover(true)}
          onPointerLeave={hover(false)}
          onClick={(event) => {
            // A click on a badge that hovering opened keeps the panel open instead of closing it.
            if (open && !pinned.current) {
              event.preventDefault();
              window.clearTimeout(timer.current);
              pinned.current = true;
            }
          }}
          aria-label={t('webSearch.badgeLabel', { hosts: hosts.join(', ') })}
          className="ms-1.5 inline-flex max-w-48 items-center gap-1 rounded-full bg-muted px-2 align-middle text-xs leading-5 font-medium text-muted-foreground transition-colors duration-(--duration-instant) ease-standard hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring data-[state=open]:bg-accent data-[state=open]:text-accent-foreground [&_svg]:size-3 [&_svg]:shrink-0"
        >
          {entries.some((entry) => !entry.known) ? <CircleAlert aria-hidden="true" /> : null}
          <span className="truncate">{hostOf(first.url)}</span>
          {entries.length > 1 ? <span className="tabular-nums">+{entries.length - 1}</span> : null}
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        onPointerEnter={hover(true)}
        onPointerLeave={hover(false)}
        // Opened by hover, the panel must not take the focus away from where the user is.
        onOpenAutoFocus={(event) => {
          if (!pinned.current) {
            event.preventDefault();
          }
        }}
        className="flex w-80 max-w-[calc(100vw-var(--spacing)*8)] flex-col gap-1 p-1.5 text-sm"
      >
        <p className="px-2 pt-1 text-xs font-medium text-muted-foreground">{t('webSearch.badgeHeading')}</p>
        <ul className="flex flex-col">
          {entries.map((entry) => (
            <li key={entry.url}>
              <a
                href={entry.url}
                target="_blank"
                rel="noopener noreferrer nofollow"
                className="flex min-w-0 flex-col gap-0.5 rounded-md px-2 py-1.5 transition-colors duration-(--duration-instant) ease-standard hover:bg-accent hover:text-accent-foreground focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
              >
                <span className="line-clamp-2 break-words font-medium">
                  <RevealedText text={entry.title} />
                </span>
                <span className="flex min-w-0 items-center gap-1 text-xs text-muted-foreground">
                  {!entry.known ? <CircleAlert className="size-3 shrink-0" aria-hidden="true" /> : null}
                  <span className="truncate">
                    {hostOf(entry.url)}
                    {!entry.known ? ` · ${t('webSearch.unchecked')}` : !entry.read ? ` · ${t('webSearch.searchHitOnly')}` : null}
                  </span>
                </span>
              </a>
            </li>
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  );
}
