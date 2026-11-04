/**
 * URLs that research mode loads without asking: they came from a search
 * provider or from a page the model read, not from the model. A URL the
 * model writes itself could carry parts of the chat to whoever runs the
 * host; one it found was written by a third party who never saw the chat.
 *
 * That only holds for the URL as found. Variants that carry nothing count as
 * found too, because models tidy URLs up: http or https, with or without
 * `www.` and a trailing slash, without tracking parameters. The query stays
 * exactly as found otherwise, raw and in order: a model choosing which of a
 * page's parameters to keep (or how to spell them) could spell out the chat
 * a bit at a time (security review H3, 3.11.).
 */
import { WEB_FETCH_TOOL, WEB_SEARCH_TOOL } from '@harness/shared';
import type { ChatMessage } from '../services/conversations.ts';

// Links as turndown writes them, including one level of parentheses in the
// URL (`…/wiki/Kaffee_(Begriffsklärung)`).
const MARKDOWN_LINK = /\]\((https?:\/\/(?:[^\s()<>]|\([^\s()<>]*\))+)\)/giu;
const BARE_URL = /\bhttps?:\/\/[^\s<>"'`()[\]{}]+/giu;
/** Links taken from one text, so a page of millions of links stays cheap. */
const MAX_LINKS_PER_TEXT = 2_000;

function parse(raw: string | URL): URL | null {
  try {
    const url = new URL(raw);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url : null;
  } catch {
    return null;
  }
}

/** Host, port and path, without scheme, `www.` and trailing slash. */
function pageKey(url: URL): string {
  const host = url.hostname.toLowerCase().replace(/^www\./, '').replace(/\.$/, '');
  const port = url.port && url.port !== (url.protocol === 'https:' ? '443' : '80') ? `:${url.port}` : '';
  return `${host}${port}${url.pathname.replace(/\/+$/, '')}`;
}

/**
 * The query's parameters as written, empty ones included: decoding would
 * let `%41` and `A` count alike, and the choice between them carry a bit.
 */
function parameters(url: URL): string[] {
  return url.search ? url.search.slice(1).split('&') : [];
}

/** Parameters that only say where a link came from; a model may leave them out. */
const TRACKING = /^(utm_[a-z]+|fbclid|gclid|dclid|gbraid|wbraid|msclkid|yclid|igshid|mc_cid|mc_eid|_ga|_gl|ref_src)(=|$)/i;

/** Whether `wanted` is `found` with some tracking parameters left out, in the same order. */
function sameQuery(found: string[], wanted: string[]): boolean {
  let next = 0;
  for (const parameter of found) {
    if (parameter === wanted[next]) {
      next += 1;
    } else if (!TRACKING.test(parameter)) {
      return false;
    }
  }
  return next === wanted.length;
}

/** See the module comment. */
export class KnownUrls {
  /** Per page key: the URLs found for it, with their query parameters. */
  readonly #pages = new Map<string, { href: string; parameters: string[] }[]>();

  /**
   * Remembers a URL from a search result or a page.
   *
   * @param raw - The URL.
   */
  add(raw: string): void {
    const url = parse(raw);
    if (!url) {
      return;
    }
    url.hash = '';
    const key = pageKey(url);
    const found = this.#pages.get(key) ?? [];
    if (!found.some((entry) => entry.href === url.href)) {
      found.push({ href: url.href, parameters: parameters(url) });
      this.#pages.set(key, found);
    }
  }

  /**
   * Remembers the links in a text the model received: Markdown links and
   * bare URLs.
   *
   * @param text - Page text.
   */
  addLinks(text: string): void {
    let count = 0;
    for (const pattern of [MARKDOWN_LINK, BARE_URL]) {
      for (const match of text.matchAll(pattern)) {
        if (count >= MAX_LINKS_PER_TEXT) {
          return;
        }
        count += 1;
        // A bare URL at the end of a sentence takes its full stop along.
        this.add((match[1] ?? match[0]).replace(/[.,;:!?]+$/, ''));
      }
    }
  }

  /**
   * @param url - URL the model wants to load.
   * @returns True if it is a found URL or a variant of one that carries nothing.
   */
  has(url: URL): boolean {
    const wanted = parameters(url);
    return (this.#pages.get(pageKey(url)) ?? []).some((entry) => sameQuery(entry.parameters, wanted));
  }

  /**
   * A found URL that looks like the requested one, so the user can compare
   * them: same page with other parameters, else the same host with the
   * longest common path.
   *
   * @param url - URL the model wants to load.
   * @returns The URL, or null if nothing on that host was found.
   */
  similar(url: URL): string | null {
    const key = pageKey(url);
    const wanted = parameters(url);
    const samePage = this.#pages.get(key) ?? [];
    if (samePage.length > 0) {
      const shared = (entry: { parameters: string[] }) => wanted.filter((parameter) => entry.parameters.includes(parameter)).length;
      return samePage.reduce((best, entry) => (shared(entry) > shared(best) ? entry : best)).href;
    }
    const host = key.split('/')[0] ?? '';
    let best: { href: string; score: number } | null = null;
    for (const [page, entries] of this.#pages) {
      if (page.split('/')[0] !== host) {
        continue;
      }
      let score = 0;
      while (score < Math.min(page.length, key.length) && page[score] === key[score]) {
        score += 1;
      }
      const entry = entries[0];
      if (entry && (!best || score > best.score)) {
        best = { href: entry.href, score };
      }
    }
    return best?.href ?? null;
  }

  /**
   * The URLs of a conversation's earlier searches and fetched pages.
   *
   * @param messages - Stored messages; tool parts only come from the server.
   * @returns The set.
   */
  static fromMessages(messages: ChatMessage[]): KnownUrls {
    const known = new KnownUrls();
    for (const message of messages) {
      if (message.role !== 'assistant') {
        continue;
      }
      for (const part of message.parts) {
        if (!('state' in part) || part.state !== 'output-available') {
          continue;
        }
        const output = (part as { output?: unknown }).output as Record<string, unknown> | undefined;
        if (part.type === `tool-${WEB_SEARCH_TOOL}` && Array.isArray(output?.results)) {
          for (const result of output.results as { url?: unknown }[]) {
            if (typeof result?.url === 'string') {
              known.add(result.url);
            }
          }
        }
        if (part.type === `tool-${WEB_FETCH_TOOL}` && typeof output?.text === 'string') {
          known.addLinks(output.text);
        }
      }
    }
    return known;
  }
}
