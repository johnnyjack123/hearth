/**
 * Search providers behind one interface: a self-hosted SearXNG, the Brave
 * Search API and Tavily. Each returns plain-text hits; the tool decides what
 * the model sees.
 */
import type { SearchProvider, WebSearchErrorCode, WebSearchResult } from '@hearth/shared';

/**
 * A web search that did not happen or failed. Like `WebFetchError`, the
 * message starts with a code the web app translates (`[code]` or
 * `[code:detail]`) and continues in English for the model.
 */
export class WebSearchError extends Error {
  /**
   * @param code - Reason, translated by the web app.
   * @param explanation - English text for the model and the server log.
   * @param detail - Short value the web app shows, e.g. an HTTP status.
   */
  constructor(
    readonly code: WebSearchErrorCode,
    explanation: string,
    readonly detail?: string,
  ) {
    super(`[${code}${detail ? `:${detail.replace(/[^\w./+-]/g, '').slice(0, 100)}` : ''}] ${explanation}`);
    this.name = 'WebSearchError';
  }
}

/** What a provider needs per search. */
export interface SearchRequest {
  /** Search terms. */
  query: string;
  /** Most hits wanted. */
  count: number;
  /** Ends the request, e.g. when the user stops the reply. */
  signal?: AbortSignal;
}

/** A configured provider, ready to search. */
export interface SearchBackend {
  /** Which provider it is. */
  provider: Exclude<SearchProvider, 'off'>;
  /**
   * Searches the web.
   *
   * @param request - See {@link SearchRequest}.
   * @returns The hits, best first, at most `count`.
   * @throws {WebSearchError} If the provider cannot be reached or refuses.
   */
  search(request: SearchRequest): Promise<WebSearchResult[]>;
}

/** Endpoints and limits; tests point the keyed providers at local servers. */
export interface SearchEndpoints {
  /** Brave Search API, web search endpoint. */
  braveUrl: string;
  /** Tavily search endpoint. */
  tavilyUrl: string;
  /** Time limit per search. */
  timeoutMs: number;
}

/** The real endpoints, 15 s per search. */
export const DEFAULT_SEARCH_ENDPOINTS: Readonly<SearchEndpoints> = Object.freeze({
  braveUrl: 'https://api.search.brave.com/res/v1/web/search',
  tavilyUrl: 'https://api.tavily.com/search',
  timeoutMs: 15_000,
});

/** Largest response body read from a provider. */
const MAX_RESPONSE_BYTES = 2 * 1024 * 1024;
const MAX_TITLE = 300;
const MAX_SNIPPET = 600;

const ENTITIES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', '#39': "'", '#x27': "'" };

/** Plain text from a provider's snippet: no tags, entities decoded, one line, bounded. */
function plain(value: unknown, max: number): string {
  if (typeof value !== 'string') {
    return '';
  }
  const text = value
    .replace(/<[^>]{0,200}>/g, '')
    .replace(/&(#?\w{1,8});/g, (match, name: string) => ENTITIES[name.toLowerCase()] ?? match)
    .replace(/\s+/g, ' ')
    .trim();
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

/** An http(s) URL as a string, or null for anything else. */
function webUrl(value: unknown): string | null {
  if (typeof value !== 'string' || value.length > 2_048) {
    return null;
  }
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : null;
  } catch {
    return null;
  }
}

/**
 * Turns the provider's raw hits into results: only http(s) URLs, each URL
 * once, plain-text title and snippet.
 *
 * @param items - Raw hits with the provider's field names already mapped.
 * @param count - Most results.
 * @returns The results.
 */
export function normalizeResults(
  items: { title: unknown; url: unknown; snippet: unknown; published: unknown }[],
  count: number,
): WebSearchResult[] {
  const seen = new Set<string>();
  const results: WebSearchResult[] = [];
  for (const item of items) {
    const url = webUrl(item.url);
    if (!url || seen.has(url)) {
      continue;
    }
    seen.add(url);
    const published = plain(item.published, 40);
    results.push({
      title: plain(item.title, MAX_TITLE) || url,
      url,
      snippet: plain(item.snippet, MAX_SNIPPET),
      published: published || null,
    });
    if (results.length >= count) {
      break;
    }
  }
  return results;
}

/** What a provider's HTTP status means for the user. */
function statusError(provider: string, status: number, rejectedHint: string): WebSearchError {
  if (status === 401 || status === 403) {
    return new WebSearchError('rejected', `${provider} rejected the request: ${rejectedHint}`, String(status));
  }
  if (status === 429) {
    return new WebSearchError('rate_limited', `${provider} allows no more searches right now (rate limit or quota).`, String(status));
  }
  return new WebSearchError('provider_error', `${provider} answered with HTTP ${status}.`, String(status));
}

/** Sends a request and reads a bounded JSON body; failures become {@link WebSearchError}s. */
async function requestJson(
  provider: string,
  url: string,
  init: RequestInit,
  timeoutMs: number,
  signal: AbortSignal | undefined,
  rejectedHint: string,
): Promise<unknown> {
  const deadline = AbortSignal.timeout(timeoutMs);
  let response: Response;
  try {
    response = await fetch(url, {
      ...init,
      redirect: 'error',
      signal: signal ? AbortSignal.any([signal, deadline]) : deadline,
    });
    if (!response.ok) {
      await response.body?.cancel();
      throw statusError(provider, response.status, rejectedHint);
    }
    const body = await readLimited(response);
    return JSON.parse(body) as unknown;
  } catch (error) {
    if (error instanceof WebSearchError) {
      throw error;
    }
    if (signal?.aborted) {
      throw new WebSearchError('aborted', 'The chat request was stopped during the search.');
    }
    if (deadline.aborted) {
      throw new WebSearchError('timeout', `${provider} did not answer in time.`);
    }
    if (error instanceof SyntaxError) {
      throw new WebSearchError('provider_error', `${provider} did not answer with JSON.`);
    }
    // fetch only says "fetch failed"; the cause tells a missing container
    // (ENOTFOUND) from one that does not listen (ECONNREFUSED).
    const cause = networkCause(error);
    console.warn(`[search] ${provider} could not be reached: ${cause ?? String(error)}`);
    throw new WebSearchError('network', `${provider} could not be reached${cause ? ` (${cause})` : ''}.`, cause ?? undefined);
  }
}

/**
 * The reason behind a failed `fetch`: a system error code such as
 * `ENOTFOUND`, or `redirect` for a redirect the request refused.
 *
 * @param error - What `fetch` threw.
 * @returns The code, or null if there is none.
 */
export function networkCause(error: unknown): string | null {
  const cause = (error as { cause?: { code?: unknown; message?: unknown } } | null)?.cause;
  if (typeof cause?.code === 'string' && /^[A-Z_]{2,40}$/.test(cause.code)) {
    return cause.code;
  }
  return /redirect/i.test(String(cause?.message ?? '')) ? 'redirect' : null;
}

async function readLimited(response: Response): Promise<string> {
  const reader = response.body?.getReader();
  if (!reader) {
    return '';
  }
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    size += value.byteLength;
    if (size > MAX_RESPONSE_BYTES) {
      await reader.cancel();
      throw new WebSearchError('provider_error', 'The search provider sent too much data.');
    }
    chunks.push(value);
  }
  return Buffer.concat(chunks).toString('utf8');
}

function array(value: unknown): Record<string, unknown>[] {
  return Array.isArray(value) ? value.filter((item): item is Record<string, unknown> => typeof item === 'object' && item !== null) : [];
}

/**
 * A SearXNG instance with JSON output switched on (`search.formats` in its
 * settings.yml); admins configure its URL, which may be a private address
 * such as the compose service `http://searxng:8080`.
 *
 * @param baseUrl - Base URL of the instance.
 * @param endpoints - Time limit.
 * @returns The backend.
 */
export function searxngBackend(baseUrl: string, endpoints: SearchEndpoints): SearchBackend {
  return {
    provider: 'searxng',
    async search({ query, count, signal }) {
      const url = new URL('search', baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`);
      url.search = new URLSearchParams({ q: query, format: 'json', safesearch: '0' }).toString();
      const body = await requestJson(
        'SearXNG',
        url.href,
        { headers: { accept: 'application/json' } },
        endpoints.timeoutMs,
        signal,
        'JSON output is probably not enabled (search.formats in settings.yml) or the limiter blocks the server.',
      );
      const items = array((body as { results?: unknown } | null)?.results).map((item) => ({
        title: item.title,
        url: item.url,
        snippet: item.content,
        published: item.publishedDate,
      }));
      const results = normalizeResults(items, count);
      const blocked = blockedEngines(body);
      if (results.length === 0 && blocked.length > 0) {
        // Without this, the model searched five to eight more times in vain
        // (benchmark of 22.10.2026); a block lasts about an hour.
        throw new WebSearchError(
          'engines_blocked',
          `The search engines behind SearXNG are blocking requests right now (${blocked.join('; ')}). ` +
            'Do not search again in this reply. Answer from what you know, say that the web search is unavailable, ' +
            'and load pages you know with web_fetch if the answer needs them.',
          blocked.map((entry) => (entry.split(':')[0] ?? '').replace(/\s+/g, '-')).join('/'),
        );
      }
      return results;
    },
  };
}

/**
 * Engines SearXNG could not ask, as "name: reason" ("brave: too many
 * requests", "duckduckgo: CAPTCHA"). Search engines block a server that
 * asks too often; SearXNG then suspends them for a while.
 *
 * @param body - The JSON answer of SearXNG.
 * @returns The engines and why, empty when all answered.
 */
export function blockedEngines(body: unknown): string[] {
  const entries = (body as { unresponsive_engines?: unknown } | null)?.unresponsive_engines;
  if (!Array.isArray(entries)) {
    return [];
  }
  return entries
    .filter((entry): entry is [unknown, unknown] => Array.isArray(entry) && entry.length >= 2)
    .map(([name, reason]) => `${String(name).slice(0, 40)}: ${String(reason).replace(/^Suspended: /, '').slice(0, 60)}`)
    .slice(0, 8);
}

/**
 * The Brave Search API (web search).
 *
 * @param apiKey - Subscription token.
 * @param endpoints - Endpoint and time limit.
 * @returns The backend.
 */
export function braveBackend(apiKey: string, endpoints: SearchEndpoints): SearchBackend {
  return {
    provider: 'brave',
    async search({ query, count, signal }) {
      const url = new URL(endpoints.braveUrl);
      url.search = new URLSearchParams({ q: query, count: String(Math.min(count, 20)) }).toString();
      const body = await requestJson(
        'Brave Search',
        url.href,
        { headers: { accept: 'application/json', 'x-subscription-token': apiKey } },
        endpoints.timeoutMs,
        signal,
        'the API key is invalid or has no access to web search.',
      );
      const web = (body as { web?: { results?: unknown } } | null)?.web;
      const items = array(web?.results).map((item) => ({
        title: item.title,
        url: item.url,
        snippet: item.description,
        published: item.page_age ?? item.age,
      }));
      return normalizeResults(items, count);
    },
  };
}

/**
 * Tavily's search API.
 *
 * @param apiKey - API key (`tvly-…`).
 * @param endpoints - Endpoint and time limit.
 * @returns The backend.
 */
export function tavilyBackend(apiKey: string, endpoints: SearchEndpoints): SearchBackend {
  return {
    provider: 'tavily',
    async search({ query, count, signal }) {
      const body = await requestJson(
        'Tavily',
        endpoints.tavilyUrl,
        {
          method: 'POST',
          headers: { accept: 'application/json', 'content-type': 'application/json', authorization: `Bearer ${apiKey}` },
          body: JSON.stringify({ query, max_results: Math.min(count, 20), search_depth: 'basic', include_answer: false }),
        },
        endpoints.timeoutMs,
        signal,
        'the API key is invalid.',
      );
      const items = array((body as { results?: unknown } | null)?.results).map((item) => ({
        title: item.title,
        url: item.url,
        snippet: item.content,
        published: item.published_date,
      }));
      return normalizeResults(items, count);
    },
  };
}
