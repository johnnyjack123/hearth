import { WEB_FETCH_TOOL, WEB_SEARCH_TOOL } from '@harness/shared';
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

/**
 * Pages the chat read or found: requested and final addresses of every web
 * fetch, and every search hit, in all messages.
 *
 * @param messages - The conversation.
 * @returns Their page keys.
 */
export function knownPageKeys(messages: ChatMessage[]): Set<string> {
  const keys = new Set<string>();
  const add = (url: unknown) => {
    const key = typeof url === 'string' ? pageKey(url) : null;
    if (key) {
      keys.add(key);
    }
  };
  for (const message of messages) {
    for (const part of message.parts) {
      if (part.type === `tool-${WEB_FETCH_TOOL}` && 'state' in part) {
        add((part.input as { url?: unknown } | undefined)?.url);
        if (part.state === 'output-available') {
          add((part.output as { url?: unknown } | undefined)?.url);
        }
      }
      if (part.type === `tool-${WEB_SEARCH_TOOL}` && 'state' in part && part.state === 'output-available') {
        const results = (part.output as { results?: unknown } | undefined)?.results;
        for (const result of Array.isArray(results) ? (results as { url?: unknown }[]) : []) {
          add(result.url);
        }
      }
    }
  }
  return keys;
}

/** Markdown links and bare addresses; trailing punctuation is no part of them. */
const LINK = /\]\((https?:\/\/[^\s)]+)\)|(https?:\/\/[^\s<>()[\]"'`]+)/g;

/**
 * Links in a reply that point to no page the chat read or found. The model
 * may have added them from memory, and such addresses are sometimes made up.
 *
 * @param message - The reply.
 * @param known - See {@link knownPageKeys}.
 * @returns The addresses, each once, in order; empty when the chat read and
 *   found nothing, since then there is nothing to check against.
 */
export function unsourcedLinks(message: ChatMessage, known: ReadonlySet<string>): string[] {
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
