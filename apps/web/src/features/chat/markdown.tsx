import { code } from '@streamdown/code';
import { math } from '@streamdown/math';
import { mermaid } from '@streamdown/mermaid';
import { useMemo } from 'react';
import { Streamdown } from 'streamdown';
import { useT } from '@/lib/preferences.tsx';

const plugins = { code, math, mermaid };

/**
 * Renders assistant Markdown with streaming support (incomplete blocks render
 * gracefully), syntax highlighting, math and Mermaid diagrams. Loaded lazily
 * because the plugins are large.
 *
 * @param props - Markdown text and whether it is still streaming.
 * @returns The rendered Markdown.
 */
export function Markdown({ children, animating }: { children: string; animating: boolean }) {
  const t = useT();
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
    <Streamdown
      plugins={plugins}
      isAnimating={animating}
      animated
      translations={translations}
      className="min-w-0 text-base leading-7 text-foreground"
    >
      {children}
    </Streamdown>
  );
}
