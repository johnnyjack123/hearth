import { z } from 'zod';
import type { InjectionFinding, WebFetchWarning } from './web-fetch.ts';

/**
 * Name of the server-side search tool of ordinary chats in research mode.
 * Its parts are stored as `tool-web_search` in the chat history.
 */
export const WEB_SEARCH_TOOL = 'web_search';

/**
 * Where the server searches: a self-hosted SearXNG (no key, queries leave
 * only through it), the Brave Search API or Tavily (both with a key), or
 * nowhere.
 */
export const SEARCH_PROVIDERS = ['off', 'searxng', 'brave', 'tavily'] as const;

/** See {@link SEARCH_PROVIDERS}. */
export type SearchProvider = (typeof SEARCH_PROVIDERS)[number];

/** Providers that need an API key. */
export const KEYED_SEARCH_PROVIDERS = ['brave', 'tavily'] as const;

/** See {@link KEYED_SEARCH_PROVIDERS}. */
export type KeyedSearchProvider = (typeof KEYED_SEARCH_PROVIDERS)[number];

/**
 * Web search settings as admins see them. API keys never leave the server;
 * only a hint of the stored key does.
 */
export interface SearchSettings {
  /** Active provider, `off` when research mode is unavailable. */
  provider: SearchProvider;
  /** Base URL of the SearXNG instance, e.g. `http://searxng:8080`. */
  searxngUrl: string | null;
  /** Hints of the stored keys (`…abcd`), null when none is stored. */
  keys: Record<KeyedSearchProvider, string | null>;
}

/**
 * Validates a change of {@link SearchSettings}. A key of `null` deletes the
 * stored one; an omitted key stays as it is.
 */
export const updateSearchSettingsSchema = z
  .object({
    provider: z.enum(SEARCH_PROVIDERS),
    searxngUrl: z
      .url({ protocol: /^https?$/ })
      .max(500)
      .nullable(),
    braveKey: z.string().trim().min(1).max(500).nullable(),
    tavilyKey: z.string().trim().min(1).max(500).nullable(),
  })
  .partial()
  .strict();

/** See {@link updateSearchSettingsSchema}. */
export type UpdateSearchSettings = z.infer<typeof updateSearchSettingsSchema>;

/** One hit of a web search. */
export interface WebSearchResult {
  /** Title of the page. */
  title: string;
  /** Address of the page; always http or https. */
  url: string;
  /** Short excerpt from the page as the provider returned it, plain text. */
  snippet: string;
  /** Publication date as the provider states it, if any. */
  published: string | null;
}

/**
 * Result of a successful `web_search` call as stored in the chat. The model
 * receives it wrapped as untrusted data, like a fetched page.
 */
export interface WebSearchOutput {
  /** The query the model searched for. */
  query: string;
  /** Who answered it. */
  provider: Exclude<SearchProvider, 'off'>;
  /** The hits, best first. */
  results: WebSearchResult[];
  /** Possible prompt injection patterns in titles and snippets. */
  warnings: WebFetchWarning[];
  /** Where the patterns of {@link warnings} matched. */
  findings: InjectionFinding[];
}

/**
 * Why a web search failed. The error text of a failed `web_search` call
 * starts with `[code]` or `[code:detail]`, like a failed web fetch.
 */
export const WEB_SEARCH_ERRORS = ['not_configured', 'rejected', 'rate_limited', 'provider_error', 'timeout', 'network', 'aborted'] as const;

/** See {@link WEB_SEARCH_ERRORS}. */
export type WebSearchErrorCode = (typeof WEB_SEARCH_ERRORS)[number];

const ERROR_PREFIX = /^\[([a-z_]+)(?::([^\]]{0,200}))?\]\s*/;

/**
 * Reads the code and detail from the error text of a failed `web_search` call.
 *
 * @param errorText - Error text of the tool part.
 * @returns Code, optional detail (e.g. the HTTP status) and the rest of the
 *   text, or null if the text has no known code.
 */
export function parseWebSearchError(
  errorText: string,
): { code: WebSearchErrorCode; detail: string | null; message: string } | null {
  const match = ERROR_PREFIX.exec(errorText);
  const code = match?.[1] as WebSearchErrorCode | undefined;
  if (!match || !code || !WEB_SEARCH_ERRORS.includes(code)) {
    return null;
  }
  return { code, detail: match[2] ?? null, message: errorText.slice(match[0].length) };
}

/**
 * Outcome of the admins' test search: how many hits came back, or why it
 * failed.
 */
export type SearchTestResult =
  | { ok: true; count: number; titles: string[] }
  | { ok: false; code: WebSearchErrorCode; detail: string | null; message: string };
