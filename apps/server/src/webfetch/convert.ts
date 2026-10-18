/**
 * Turns a fetched body into text a model can read: HTML becomes Markdown,
 * other textual types stay as they are.
 */
import TurndownService from 'turndown';

/** The parts of a DOM element the Turndown rules below use. */
interface Element {
  getAttribute(name: string): string | null;
}

/**
 * Picks the charset of a body: the Content-Type header, then a byte order
 * mark, then a `<meta charset>` near the start of an HTML page, else UTF-8.
 *
 * @param body - Raw bytes.
 * @param headerCharset - Charset from the Content-Type header.
 * @param isHtml - Whether to look for a meta tag.
 * @returns The decoded text.
 */
export function decodeBody(body: Buffer, headerCharset: string | null, isHtml: boolean): string {
  let label = headerCharset;
  if (!label) {
    if (body[0] === 0xfe && body[1] === 0xff) {
      label = 'utf-16be';
    } else if (body[0] === 0xff && body[1] === 0xfe) {
      label = 'utf-16le';
    } else if (isHtml) {
      const head = body.subarray(0, 4_096).toString('latin1');
      label = /<meta[^>]+charset\s*=\s*["']?([\w.:-]+)/i.exec(head)?.[1] ?? null;
    }
  }
  try {
    return new TextDecoder(label ?? 'utf-8').decode(body);
  } catch {
    // Unknown labels throw; UTF-8 is the best guess on today's web.
    return new TextDecoder('utf-8').decode(body);
  }
}

const NAMED_ENTITIES: Record<string, string> = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: ' ',
  ndash: '–',
  mdash: '—',
  hellip: '…',
  laquo: '«',
  raquo: '»',
  bdquo: '„',
  ldquo: '“',
  rdquo: '”',
  lsquo: '‘',
  rsquo: '’',
  middot: '·',
  auml: 'ä',
  ouml: 'ö',
  uuml: 'ü',
  Auml: 'Ä',
  Ouml: 'Ö',
  Uuml: 'Ü',
  szlig: 'ß',
};

function decodeEntities(text: string): string {
  return text.replace(/&(#x[0-9a-f]{1,6}|#\d{1,7}|[a-z]{2,8});/gi, (match, name: string) => {
    if (name.startsWith('#')) {
      const code = name[1] === 'x' || name[1] === 'X' ? Number.parseInt(name.slice(2), 16) : Number(name.slice(1));
      return code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : match;
    }
    return NAMED_ENTITIES[name] ?? match;
  });
}

/**
 * The `<title>` of an HTML page.
 *
 * @param html - The page.
 * @returns The title in one line, at most 300 characters, or null.
 */
export function htmlTitle(html: string): string | null {
  const raw = /<title[^>]*>([\s\S]{0,2000}?)<\/title\s*>/i.exec(html)?.[1];
  const title = raw ? decodeEntities(raw.replace(/<[^>]*>/g, '')).replace(/\s+/g, ' ').trim() : '';
  return title ? title.slice(0, 300) : null;
}

// Elements without readable text, or with text that only adds noise.
const DROPPED = [
  'head',
  'title',
  'script',
  'style',
  'noscript',
  'template',
  'svg',
  'canvas',
  'iframe',
  'object',
  'embed',
  'audio',
  'video',
  'nav',
  'form',
  'button',
  'select',
  'input',
  'textarea',
] as const;

function createTurndown(base: URL): TurndownService {
  const service = new TurndownService({ headingStyle: 'atx', codeBlockStyle: 'fenced', bulletListMarker: '-' });
  service.remove([...DROPPED]);
  // Images only by their description: image URLs (and data: URIs of
  // megabytes) are noise for a text model.
  service.addRule('image', {
    filter: 'img',
    replacement: (_content, node) => {
      const alt = (node as unknown as Element).getAttribute('alt')?.trim();
      return alt ? `[Image: ${alt}]` : '';
    },
  });
  // Relative links are useless out of context; script and data links too.
  service.addRule('link', {
    filter: (node) => node.nodeName === 'A' && Boolean((node as unknown as Element).getAttribute('href')),
    replacement: (content, node) => {
      const text = content.trim();
      try {
        const href = new URL((node as unknown as Element).getAttribute('href') ?? '', base);
        if (href.protocol !== 'http:' && href.protocol !== 'https:') {
          return text;
        }
        return text ? `[${text}](${href.href})` : '';
      } catch {
        return text;
      }
    },
  });
  return service;
}

// Script and style bodies can be most of a page; dropping them before the
// DOM is built keeps the conversion fast on small servers.
const HEAVY_BLOCKS = /<(script|style|svg|noscript|template)\b[^>]*>[\s\S]*?<\/\1\s*>/gi;

/**
 * Converts an HTML page to Markdown.
 *
 * @param html - The page.
 * @param base - URL of the page, for relative links.
 * @returns Markdown without scripts, styles, navigation and images.
 */
export function htmlToMarkdown(html: string, base: URL): string {
  return createTurndown(base).turndown(html.replace(HEAVY_BLOCKS, ''));
}

/**
 * Tidies text for the model: unified line breaks, no runs of blank lines,
 * no invisible Unicode tag characters (they only serve to hide text).
 *
 * @param text - Readable text.
 * @returns The tidied text.
 */
export function tidy(text: string): string {
  return text
    .replace(/\r\n?/g, '\n')
    .replace(/[\u{E0000}-\u{E007F}]/gu, '')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

/**
 * Shortens text to a number of characters, preferably at a line break.
 *
 * @param text - Text.
 * @param maxChars - Limit.
 * @returns The text and whether it was cut.
 */
export function truncate(text: string, maxChars: number): { text: string; truncated: boolean } {
  if (text.length <= maxChars) {
    return { text, truncated: false };
  }
  const cut = text.slice(0, maxChars);
  const lineBreak = cut.lastIndexOf('\n');
  return { text: lineBreak > maxChars * 0.8 ? cut.slice(0, lineBreak) : cut, truncated: true };
}
