/**
 * Turns a fetched body into text a model can read: HTML becomes Markdown,
 * preferably only its main content, other textual types stay as they are.
 */
import { Readability } from '@mozilla/readability';
import { parseHTML } from 'linkedom';
import TurndownService from 'turndown';

/** The parts of a DOM node the Turndown rules below use. */
interface Element {
  nodeName: string;
  nodeType: number;
  parentNode: Element | null;
  previousSibling: Element | null;
  childNodes: ArrayLike<Element>;
  getAttribute(name: string): string | null;
}

const ELEMENT_NODE = 1;

/** The element siblings before `node`, e.g. the cells left of a cell. */
function elementsBefore(node: Element): number {
  let count = 0;
  for (let sibling = node.previousSibling; sibling; sibling = sibling.previousSibling) {
    if (sibling.nodeType === ELEMENT_NODE) {
      count += 1;
    }
  }
  return count;
}

const TABLE_SECTIONS = new Set(['THEAD', 'TBODY', 'TFOOT']);

/** Whether `row` is the first row of its table, the one Markdown needs as a header. */
function isFirstRow(row: Element): boolean {
  let table = row.parentNode;
  while (table && table.nodeName !== 'TABLE') {
    table = table.parentNode;
  }
  for (const child of Array.from(table?.childNodes ?? [])) {
    if (child.nodeName === 'TR') {
      return child === row;
    }
    if (TABLE_SECTIONS.has(child.nodeName)) {
      const first = Array.from(child.childNodes).find((node) => node.nodeName === 'TR');
      if (first) {
        return first === row;
      }
    }
  }
  return false;
}

function cellCount(row: Element): number {
  return Array.from(row.childNodes).filter((node) => node.nodeName === 'TH' || node.nodeName === 'TD').length;
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
  // Tables as Markdown tables, the first row as header; otherwise every cell
  // ends up on a line of its own and the columns are lost.
  service.addRule('tableCell', {
    filter: ['th', 'td'],
    replacement: (content, node) => {
      const cell = content.replace(/\s*\n+\s*/g, ' ').replace(/\|/g, '\\|').trim();
      return `${elementsBefore(node as unknown as Element) === 0 ? '| ' : ' '}${cell} |`;
    },
  });
  service.addRule('tableRow', {
    filter: 'tr',
    replacement: (content, node) => {
      const row = node as unknown as Element;
      const separator = isFirstRow(row) ? `\n|${' --- |'.repeat(Math.max(1, cellCount(row)))}` : '';
      return `\n${content}${separator}`;
    },
  });
  service.addRule('table', {
    filter: 'table',
    replacement: (content) => `\n\n${content.replace(/\n{2,}/g, '\n').trim()}\n\n`,
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
 * Less text than this from Readability means it probably picked the wrong
 * part (or the page is a form, a link list …): then the whole page is
 * better.
 */
const MIN_MAIN_CHARS = 500;

/**
 * Main content with less than this share of the page's text is probably a
 * box on a page without an article, such as a teaser on a news front page;
 * then the whole page is better. Measured on 18.10.2026: front pages of
 * heise and SPIEGEL 0.12 and 0.07, articles, docs and READMEs 0.27 to 0.96.
 */
const MIN_MAIN_SHARE = 0.2;

/** Pages with more elements are converted whole; Readability would take too long on a small server. */
const MAX_ELEMENTS = 30_000;

function textLength(text: string | null | undefined): number {
  return (text ?? '').replace(/\s+/g, ' ').trim().length;
}

/**
 * Converts the main content of an HTML page to Markdown: the article as
 * Mozilla Readability finds it, without navigation, sidebars, footers and
 * hidden elements. Everything runs locally; an external reader service would
 * see every URL the model fetches.
 *
 * @param html - The page.
 * @param base - URL of the page, for relative links.
 * @returns Markdown of the main content, or null if Readability found none
 *   with at least {@link MIN_MAIN_CHARS} characters and
 *   {@link MIN_MAIN_SHARE} of the page's text.
 */
export function mainContentToMarkdown(html: string, base: URL): string | null {
  try {
    const { document } = parseHTML(html.replace(HEAVY_BLOCKS, ''));
    // Measured before Readability, which takes the document apart.
    const pageChars = textLength(document.body?.textContent);
    const article = new Readability(document, { maxElemsToParse: MAX_ELEMENTS, charThreshold: MIN_MAIN_CHARS }).parse();
    const mainChars = textLength(article?.textContent);
    if (!article?.content || mainChars < MIN_MAIN_CHARS || mainChars < pageChars * MIN_MAIN_SHARE) {
      return null;
    }
    // Readability moves the heading and the byline out of the content; the
    // date in a byline tells the model how current the page is.
    const title = article.title?.replace(/\s+/g, ' ').trim();
    const byline = article.byline?.replace(/\s+/g, ' ').trim();
    const header = [title ? `# ${title}` : '', byline ?? '', article.publishedTime ? `Published: ${article.publishedTime}` : ''];
    return [...header.filter(Boolean), createTurndown(base).turndown(article.content)].join('\n\n');
  } catch {
    // Readability gives up on large documents by throwing; odd markup can
    // trip linkedom. The whole page still works.
    return null;
  }
}

/**
 * Converts an HTML page for the model: its main content, or the whole page
 * when asked for or when no main content stands out.
 *
 * @param html - The page.
 * @param base - URL of the page, for relative links.
 * @param fullPage - Skip the main content and convert everything.
 * @returns The Markdown and whether it is only the main content.
 */
export function pageToMarkdown(html: string, base: URL, fullPage = false): { markdown: string; mainContent: boolean } {
  const main = fullPage ? null : mainContentToMarkdown(html, base);
  return main === null ? { markdown: htmlToMarkdown(html, base), mainContent: false } : { markdown: main, mainContent: true };
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
