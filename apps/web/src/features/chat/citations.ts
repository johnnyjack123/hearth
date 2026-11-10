import { WEB_FETCH_TOOL, WEB_SEARCH_TOOL } from '@hearth/shared';
import type { ChatMessage } from '@/lib/api.ts';

/**
 * Host and path of a URL, without scheme, `www.`, query, fragment and
 * trailing slash: models link a page in a tidied-up variant of the address
 * they loaded.
 *
 * @param url - Any address.
 * @returns The key, or null if it is no http(s) URL.
 */
export function pageKey(url: string): string | null {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      return null;
    }
    return `${parsed.host.toLowerCase().replace(/^www\./, '')}${parsed.pathname.replace(/\/+$/, '')}`;
  } catch {
    return null;
  }
}

/** A page the chat read or found, as a reply can cite it. */
export interface KnownSource {
  /** Address after redirects, or of the search hit. */
  url: string;
  /** Page title or title of the hit; comes from the page, so untrusted. */
  title: string;
  /** True if the model loaded the page, false for a search hit it did not read. */
  read: boolean;
}

/** Text of a stored field, or null; tool parts come from the server's history. */
function text(value: unknown): string | null {
  return typeof value === 'string' && value ? value : null;
}

/**
 * Pages the chat read or found: requested and final addresses of every web
 * fetch, and every search hit, in all messages. A page that was read wins
 * over the same address as a mere hit.
 *
 * @param messages - The conversation.
 * @returns The pages by {@link pageKey}.
 */
export function knownSources(messages: ChatMessage[]): Map<string, KnownSource> {
  const sources = new Map<string, KnownSource>();
  const add = (url: string | null, source: KnownSource) => {
    const key = url ? pageKey(url) : null;
    if (key && (source.read || !sources.has(key))) {
      sources.set(key, source);
    }
  };
  for (const message of messages) {
    for (const part of message.parts) {
      if (part.type === `tool-${WEB_SEARCH_TOOL}` && 'state' in part && part.state === 'output-available') {
        const results = (part.output as { results?: unknown } | undefined)?.results;
        for (const result of Array.isArray(results) ? (results as { url?: unknown; title?: unknown }[]) : []) {
          const url = text(result.url);
          if (url) {
            add(url, { url, title: text(result.title) ?? url, read: false });
          }
        }
      }
    }
  }
  for (const message of messages) {
    for (const part of message.parts) {
      if (part.type === `tool-${WEB_FETCH_TOOL}` && 'state' in part && part.state === 'output-available') {
        const output = part.output as { url?: unknown; title?: unknown } | undefined;
        const url = text(output?.url);
        if (url) {
          const source = { url, title: text(output?.title) ?? url, read: true };
          add(url, source);
          add(text((part.input as { url?: unknown } | undefined)?.url), source);
        }
      }
    }
  }
  return sources;
}

/**
 * A text that changes exactly when {@link knownSources} does, so callers can
 * keep the same map while a reply streams.
 *
 * @param sources - The known pages.
 * @returns The signature.
 */
export function sourcesSignature(sources: ReadonlyMap<string, KnownSource>): string {
  return [...sources].map(([key, source]) => `${key}\t${source.url}\t${source.title}\t${source.read}`).join('\n');
}

/** Markdown links and bare addresses; trailing punctuation is no part of them. */
const LINK = /\]\((https?:\/\/[^\s)]+)\)|(https?:\/\/[^\s<>()[\]"'`]+)/g;

/**
 * Links in a reply that point to no page the chat read or found. The model
 * may have added them from memory, and such addresses are sometimes made up.
 *
 * @param message - The reply.
 * @param known - See {@link knownSources}.
 * @returns The addresses, each once, in order; empty when the chat read and
 *   found nothing, since then there is nothing to check against.
 */
export function unsourcedLinks(message: ChatMessage, known: ReadonlyMap<string, unknown>): string[] {
  if (known.size === 0) {
    return [];
  }
  const links: string[] = [];
  for (const part of message.parts) {
    if (part.type !== 'text') {
      continue;
    }
    for (const match of part.text.matchAll(LINK)) {
      const url = (match[1] ?? match[2] ?? '').replace(/[.,;:!?]+$/, '');
      const key = pageKey(url);
      if (key && !known.has(key) && !links.includes(url)) {
        links.push(url);
      }
    }
  }
  return links;
}

/** Tag of the element that {@link rehypeSourceBadges} puts at the end of a paragraph. */
export const SOURCE_BADGE_TAG = 'source-badge';

/** The parts of a hast tree the plugin touches; the rest passes through. */
interface HastText {
  type: 'text';
  value: string;
}
interface HastElement {
  type: 'element';
  tagName: string;
  properties?: Record<string, unknown>;
  children: HastNode[];
}
type HastNode = HastText | HastElement | { type: 'root' | 'comment' | 'doctype' | 'raw'; children?: HastNode[] };

/** Elements whose inline content can end with sources. */
const PARAGRAPHS = new Set(['p', 'li', 'td', 'th']);
/** Children of a list item that end its own text. */
const NESTED = new Set(['p', 'ul', 'ol', 'blockquote', 'pre', 'table', 'div', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6']);
/** What may stand between the links of one group of sources. */
const SEPARATOR = /^\s*(?:[,;&]|und|and|sowie)?\s*$/i;

function isText(node: HastNode | undefined): node is HastText {
  return node?.type === 'text';
}

function linkUrl(node: HastNode | undefined): string | null {
  if (node?.type !== 'element' || node.tagName !== 'a') {
    return null;
  }
  const href = node.properties?.href;
  return typeof href === 'string' && /^https?:\/\//i.test(href) ? href : null;
}

/**
 * Moves the sources of one paragraph into a badge at its end: groups of links
 * in brackets, or at the very end, with at least one known page among them.
 * Links in the middle of a sentence stay where they are; known ones are also
 * listed in the badge.
 */
function collectSources(element: HastElement, isSource: (url: string) => boolean) {
  const nested = element.tagName === 'li' ? element.children.findIndex((child) => child.type === 'element' && NESTED.has(child.tagName)) : -1;
  const end = nested === -1 ? element.children.length : nested;
  const kids = element.children.slice(0, end);
  const urls: string[] = [];
  let index = 0;
  while (index < kids.length) {
    if (!linkUrl(kids[index])) {
      index += 1;
      continue;
    }
    let last = index;
    while (true) {
      const next = kids[last + 1];
      if (linkUrl(next)) {
        last += 1;
      } else if (isText(next) && SEPARATOR.test(next.value) && linkUrl(kids[last + 2])) {
        last += 2;
      } else {
        break;
      }
    }
    const links = kids.slice(index, last + 1).flatMap((node) => linkUrl(node) ?? []);
    if (!links.some(isSource)) {
      index = last + 1;
      continue;
    }
    const before = kids[index - 1];
    const after = kids[last + 1];
    const bracketed = isText(before) && /[([]\s*$/.test(before.value) && isText(after) && /^\s*[)\]]/.test(after.value);
    const trailing = kids.slice(last + 1).every((node) => isText(node) && /^[\s.,;:!?)\]]*$/.test(node.value));
    if (!bracketed && !trailing) {
      urls.push(...links.filter(isSource));
      index = last + 1;
      continue;
    }
    if (bracketed) {
      before.value = before.value.replace(/\s*[([]\s*$/, '');
      after.value = after.value.replace(/^\s*[)\]]/, '');
    } else if (isText(before)) {
      before.value = before.value.replace(/\s*(?:(?:quellen?|sources?)\s*:)?\s*$/i, '');
    }
    urls.push(...links);
    kids.splice(index, last - index + 1);
  }
  const seen = new Set<string>();
  const unique = urls.filter((url) => {
    const key = pageKey(url) ?? url;
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
  if (unique.length > 0) {
    kids.push({ type: 'element', tagName: SOURCE_BADGE_TAG, properties: { dataUrls: JSON.stringify(unique) }, children: [] });
  }
  element.children = [...kids, ...element.children.slice(end)];
}

function visit(node: HastNode, isSource: (url: string) => boolean) {
  if (node.type === 'element' && PARAGRAPHS.has(node.tagName)) {
    collectSources(node, isSource);
  }
  for (const child of ('children' in node ? node.children : undefined) ?? []) {
    visit(child, isSource);
  }
}

/**
 * Rehype plugin for replies: the sources at the end of a paragraph or list
 * item become one {@link SOURCE_BADGE_TAG} element with their addresses in
 * `data-urls`, so the text reads without a trail of links. It has to run
 * after sanitizing, which would drop the unknown element.
 *
 * @param options - Which addresses are pages the chat read or found.
 * @returns The transformer.
 */
export function rehypeSourceBadges(options: { isSource: (url: string) => boolean }) {
  return (tree: HastNode) => visit(tree, options.isSource);
}
