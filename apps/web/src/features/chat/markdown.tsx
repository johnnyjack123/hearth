import { code } from '@streamdown/code';
import { math } from '@streamdown/math';
import { mermaid } from '@streamdown/mermaid';
import { type ComponentProps, useMemo } from 'react';
import { type Components, defaultRehypePlugins, type ExtraProps, Streamdown } from 'streamdown';
import { useT } from '@/lib/preferences.tsx';
import { type KnownSource, pageKey, rehypeSourceBadges, SOURCE_BADGE_TAG } from './citations.ts';
import { normalizeMath } from './math.ts';
import { SourceBadge, SourcesContext } from './source-badge.tsx';

const plugins = { code, math, mermaid };

/**
 * A real link instead of Streamdown's button with a confirmation dialog: it
 * shows its target on hover, opens in a new tab and works with middle click
 * and "copy link". Anything but http(s) and mailto stays text, including the
 * placeholder of a link that is still streaming.
 */
function MarkdownLink({ href, children, title }: ComponentProps<'a'> & ExtraProps) {
  if (!href || !/^(?:https?:|mailto:)/i.test(href)) {
    return <span>{children}</span>;
  }
  return (
    <a
      href={href}
      title={title}
      target="_blank"
      rel="noopener noreferrer nofollow"
      className="wrap-anywhere font-medium text-primary underline decoration-primary/40 underline-offset-2 transition-colors duration-(--duration-instant) ease-standard hover:decoration-primary focus-visible:rounded-xs focus-visible:outline-none focus-visible:ring-(length:--focus-ring-width) focus-visible:ring-ring"
    >
      {children}
    </a>
  );
}

const components: Components = { a: MarkdownLink, [SOURCE_BADGE_TAG]: SourceBadge };
const NO_SOURCES: ReadonlyMap<string, KnownSource> = new Map();

/**
 * Renders assistant Markdown with streaming support (incomplete blocks render
 * gracefully), syntax highlighting, math and Mermaid diagrams. Links to pages
 * the chat read or found that close a paragraph become a source badge at its
 * end. Loaded lazily because the plugins are large.
 *
 * @param props - Markdown text, whether it is still streaming, and the pages
 *   the chat read or found (keep the same map while nothing changes, every
 *   new one renders the whole reply again).
 * @returns The rendered Markdown.
 */
export function Markdown({
  children,
  animating,
  sources = NO_SOURCES,
}: {
  children: string;
  animating: boolean;
  sources?: ReadonlyMap<string, KnownSource>;
}) {
  const t = useT();
  const markdown = useMemo(() => normalizeMath(children), [children]);
  const rehypePlugins = useMemo(
    () => [
      ...Object.values(defaultRehypePlugins),
      [rehypeSourceBadges, { isSource: (url: string) => sources.has(pageKey(url) ?? '') }] as [
        typeof rehypeSourceBadges,
        Parameters<typeof rehypeSourceBadges>[0],
      ],
    ],
    [sources],
  );
  const translations = useMemo(
    () => ({
      copyCode: t('markdown.copyCode'),
      copied: t('markdown.copied'),
      copyLink: t('markdown.copyLink'),
      openLink: t('markdown.openLink'),
      openExternalLink: t('markdown.openExternalLink'),
      externalLinkWarning: t('markdown.externalLinkWarning'),
      downloadFile: t('markdown.downloadFile'),
      downloadImage: t('markdown.downloadImage'),
      imageNotAvailable: t('markdown.imageNotAvailable'),
      copyTable: t('markdown.copyTable'),
      downloadTable: t('markdown.downloadTable'),
      downloadDiagram: t('markdown.downloadDiagram'),
      viewFullscreen: t('markdown.viewFullscreen'),
      exitFullscreen: t('markdown.exitFullscreen'),
      zoomIn: t('markdown.zoomIn'),
      zoomOut: t('markdown.zoomOut'),
      resetView: t('markdown.resetView'),
      close: t('markdown.close'),
    }),
    [t],
  );

  return (
    <SourcesContext.Provider value={sources}>
      <Streamdown
        plugins={plugins}
        components={components}
        rehypePlugins={rehypePlugins}
        isAnimating={animating}
        animated
        translations={translations}
        className="min-w-0 text-base leading-7 text-foreground"
      >
        {markdown}
      </Streamdown>
    </SourcesContext.Provider>
  );
}
