/**
 * URLs that research mode loads without asking: they came from a search
 * provider or from a page the model read, not from the model. A URL the
 * model writes itself could carry parts of the chat to whoever runs the
 * host; one it found was written by a third party who never saw the chat.
 * That only holds for the exact URL, so the set compares whole URLs, never
 * hosts.
 */
import { WEB_FETCH_TOOL, WEB_SEARCH_TOOL } from '@harness/shared';
import type { ChatMessage } from '../services/conversations.ts';

// Links as turndown writes them, including one level of parentheses in the
// URL (`…/wiki/Kaffee_(Begriffsklärung)`).
const MARKDOWN_LINK = /\]\((https?:\/\/(?:[^\s()<>]|\([^\s()<>]*\))+)\)/giu;
const BARE_URL = /\bhttps?:\/\/[^\s<>"'`()[\]{}]+/giu;
/** Links taken from one text, so a page of millions of links stays cheap. */
const MAX_LINKS_PER_TEXT = 2_000;

/**
 * The comparison key of a URL: parsed, without fragment.
 *
 * @param raw - URL as found or as requested.
 * @returns The key, or null for anything but http(s).
 */
export function urlKey(raw: string | URL): string | null {
  try {
    const url = new URL(raw);
    if (url.protocol !== 'http:' && url.protocol !== 'https:') {
      return null;
    }
    url.hash = '';
    return url.href;
  } catch {
    return null;
  }
}

/** See the module comment. */
export class KnownUrls {
  readonly #urls = new Set<string>();

  /**
   * Remembers a URL from a search result or a page.
   *
   * @param raw - The URL.
   */
  add(raw: string): void {
    const key = urlKey(raw);
    if (key) {
      this.#urls.add(key);
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
   * @returns True if exactly this URL was found before.
   */
  has(url: URL): boolean {
    const key = urlKey(url);
    return key !== null && this.#urls.has(key);
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
