/**
 * OpenRouter specifics shared by ordinary chats and the pass-through for
 * agents: each request goes to the host the user pinned or else the one
 * that is cheapest right now, and what OpenRouter billed is read back from
 * its answer.
 */
import { createHash } from 'node:crypto';
import type { OpenRouterHost, TokenRates } from '@harness/shared';
import type { JSONValue } from 'ai';
import type { EffortOptions } from './effort.ts';
import type { BilledCost } from './pricing.ts';

/** Endpoint lists stay this long; prices and discounts change rarely. */
const ENDPOINTS_TTL_MS = 10 * 60_000;

/** The list of providers (where they sit) stays a day; it rarely changes. */
const PROVIDERS_TTL_MS = 24 * 60 * 60_000;

/** After a failed fetch, the next try waits this long. */
const RETRY_MS = 60_000;

/** A request waits at most this long for the list. */
const FETCH_TIMEOUT_MS = 5_000;

/** Hosts named in `provider.order`; OpenRouter falls back to the rest. */
const MAX_ORDER = 8;

/**
 * A conversation goes back to the host of its last call for this long after
 * it: that host holds the conversation in its prompt cache, any other host
 * reads the whole context at full price. Caches of the hosts last minutes to
 * hours; after half an hour without a call the cheapest host is worth more.
 */
const STICKY_MS = 30 * 60_000;

/** Conversations remembered at most; the oldest go first. */
const MAX_STICKY = 5_000;

/**
 * The call prices are compared on: most input from the cache (agent chats
 * read ~99 % of it from there, 31.10.), output a few percent of the input.
 * Comparing list prices alone would favor hosts without a cache, which
 * cost an agent ten times as much.
 */
const MIX = { input: 0.1, cacheRead: 0.9, output: 0.05 };

/** Quantizations of hosts that run a model in 4 bits. */
const FOUR_BIT = new Set(['fp4', 'mxfp4', 'nvfp4', 'int4']);

/** Model ids whose endpoints can be listed; variants (`:free`, `:nitro`) route by their own rules. */
const ROUTABLE_MODEL = /^[a-z0-9][\w.-]*\/[\w.-]+$/i;

/** Country codes as OpenRouter's provider list gives them. */
const COUNTRY = /^[A-Z]{2}$/;

/** Routing fields of a request's `provider` that a pinned host replaces. */
const ROUTING_FIELDS = ['order', 'only', 'ignore', 'sort', 'allow_fallbacks'];

/**
 * Whether OpenRouter lists hosts for a model id, so the user can pin one.
 *
 * @param modelId - OpenRouter's model id.
 * @returns True for plain ids without a variant.
 */
export function routableModel(modelId: string): boolean {
  return ROUTABLE_MODEL.test(modelId);
}

const WEEKDAYS = ['sunday', 'monday', 'tuesday', 'wednesday', 'thursday', 'friday', 'saturday'];

interface RawPrice {
  prompt?: unknown;
  completion?: unknown;
  input_cache_read?: unknown;
}

interface RawOverride extends RawPrice {
  utc_days?: unknown;
  utc_start?: unknown;
  utc_end?: unknown;
}

interface RawEndpoint {
  tag?: unknown;
  provider_name?: unknown;
  service_tier?: unknown;
  status?: unknown;
  uptime_last_30m?: unknown;
  quantization?: unknown;
  pricing?: RawPrice & { overrides?: unknown };
}

function price(value: unknown): number | undefined {
  const number = typeof value === 'string' ? Number(value) : value;
  return typeof number === 'number' && Number.isFinite(number) && number >= 0 ? number : undefined;
}

/** Whether an override (hours as HHMM in UTC, 0 as end meaning midnight) applies at `at`. */
function overrideApplies(override: RawOverride, at: Date): boolean {
  if (Array.isArray(override.utc_days) && !override.utc_days.includes(WEEKDAYS[at.getUTCDay()])) {
    return false;
  }
  const start = typeof override.utc_start === 'number' ? override.utc_start : 0;
  const end = typeof override.utc_end === 'number' && override.utc_end !== 0 ? override.utc_end : 2400;
  const now = at.getUTCHours() * 100 + at.getUTCMinutes();
  return start <= end ? now >= start && now < end : now >= start || now < end;
}

/**
 * A host's prices at a moment, its discount included (OpenRouter's prices
 * already carry it) and its hours (DeepSeek halves its prices at night).
 *
 * @param endpoint - One entry of OpenRouter's endpoint list.
 * @param at - The moment.
 * @returns Dollars per token, or null without a prompt and completion price.
 */
export function endpointPrices(endpoint: RawEndpoint, at: Date): { prompt: number; completion: number; cacheRead?: number } | null {
  const base = endpoint.pricing ?? {};
  let prices: RawPrice = base;
  if (Array.isArray(base.overrides)) {
    const found = (base.overrides as RawOverride[]).find((override) => override && typeof override === 'object' && overrideApplies(override, at));
    if (found) {
      prices = { ...base, ...found };
    }
  }
  const prompt = price(prices.prompt);
  const completion = price(prices.completion);
  if (prompt === undefined || completion === undefined) {
    return null;
  }
  const cacheRead = price(prices.input_cache_read);
  return { prompt, completion, ...(cacheRead !== undefined ? { cacheRead } : {}) };
}

/**
 * The hosts of a model from cheapest to dearest for a typical call (see
 * {@link MIX}). Hosts that are down or failed often in the last half hour
 * are left out; OpenRouter still falls back to them.
 *
 * @param raw - OpenRouter's answer of `/models/<id>/endpoints`.
 * @param at - When the call goes out.
 * @param options - `noFourBit` leaves out hosts that run the model in 4 bits.
 * @returns Endpoint tags for `provider.order`, at most {@link MAX_ORDER}.
 */
export function rankEndpoints(raw: unknown, at: Date, options: { noFourBit: boolean }): string[] {
  return rankHosts(raw, at, options)
    .slice(0, MAX_ORDER)
    .map((endpoint) => endpoint.tag);
}

/** All eligible endpoints from cheapest to dearest, with the host each belongs to. */
function rankHosts(raw: unknown, at: Date, options: { noFourBit: boolean }): { tag: string; host: string | null }[] {
  const endpoints = (raw as { data?: { endpoints?: unknown } } | null)?.data?.endpoints;
  if (!Array.isArray(endpoints)) {
    return [];
  }
  const ranked: { tag: string; host: string | null; score: number; index: number }[] = [];
  endpoints.forEach((entry: RawEndpoint, index) => {
    if (!entry || typeof entry !== 'object' || typeof entry.tag !== 'string' || !entry.tag) {
      return;
    }
    if (typeof entry.status === 'number' && entry.status < 0) {
      return;
    }
    if (typeof entry.uptime_last_30m === 'number' && entry.uptime_last_30m < 80) {
      return;
    }
    // Flex is a discounted tier OpenRouter only uses on request (`:floor`,
    // `service_tier`), without promises on waiting or capacity; an agent
    // waiting in a queue costs the user more time than it saves.
    if (isFlex(entry)) {
      return;
    }
    if (options.noFourBit && typeof entry.quantization === 'string' && FOUR_BIT.has(entry.quantization.toLowerCase())) {
      return;
    }
    const prices = endpointPrices(entry, at);
    if (!prices) {
      return;
    }
    const score = MIX.input * prices.prompt + MIX.cacheRead * (prices.cacheRead ?? prices.prompt) + MIX.output * prices.completion;
    ranked.push({ tag: entry.tag, host: typeof entry.provider_name === 'string' ? entry.provider_name : null, score, index });
  });
  // On a tie OpenRouter's own order stands.
  ranked.sort((a, b) => a.score - b.score || a.index - b.index);
  return ranked;
}

/** Model ids have no spaces, so the pair cannot be mistaken for another. */
function stickyKey(modelId: string, conversation: string): string {
  return `${modelId} ${conversation}`;
}

function isFlex(entry: RawEndpoint): boolean {
  return entry.service_tier === 'flex' || (typeof entry.tag === 'string' && /\/flex$/i.test(entry.tag));
}

/**
 * The list prices of the host that ran a call, as OpenRouter names it in its
 * answer (`provider`), at the moment of the call. A host with several
 * endpoints (quantizations) counts with its cheapest one, as the routing
 * picks it; flex endpoints are left out like there.
 *
 * @param raw - OpenRouter's answer of `/models/<id>/endpoints`.
 * @param host - The host's name, e.g. `DeepInfra`.
 * @param at - When the call ran.
 * @returns US dollars per million tokens, or null if the list does not know the host.
 */
export function hostRates(raw: unknown, host: string, at: Date): TokenRates | null {
  const endpoints = (raw as { data?: { endpoints?: unknown } } | null)?.data?.endpoints;
  if (!Array.isArray(endpoints)) {
    return null;
  }
  let best: { prompt: number; completion: number; cacheRead?: number } | null = null;
  let bestScore = Infinity;
  for (const entry of endpoints as RawEndpoint[]) {
    if (!entry || typeof entry !== 'object' || entry.provider_name !== host || isFlex(entry)) {
      continue;
    }
    const prices = endpointPrices(entry, at);
    if (!prices) {
      continue;
    }
    const score = MIX.input * prices.prompt + MIX.cacheRead * (prices.cacheRead ?? prices.prompt) + MIX.output * prices.completion;
    if (score < bestScore) {
      best = prices;
      bestScore = score;
    }
  }
  return best ? perMillion(best) : null;
}

/** Per token in the list, per million tokens everywhere else. */
function perMillion(prices: { prompt: number; completion: number; cacheRead?: number }): TokenRates {
  const million = (value: number) => Math.round(value * 1e6 * 1e9) / 1e9;
  return {
    input: million(prices.prompt),
    output: million(prices.completion),
    ...(prices.cacheRead !== undefined ? { cacheRead: million(prices.cacheRead) } : {}),
  };
}

/**
 * The hosts of a model for the user to pick from, one per provider: its
 * cheapest endpoint's prices now, its quantizations, and where the company
 * sits and runs its data centers. Hosts that are down or failed often in
 * the last half hour come last.
 *
 * @param raw - OpenRouter's answer of `/models/<id>/endpoints`.
 * @param providers - OpenRouter's answer of `/providers`, or null.
 * @param at - The moment the prices are for.
 * @returns The hosts, cheapest first.
 */
export function hostChoices(raw: unknown, providers: unknown, at: Date): OpenRouterHost[] {
  const endpoints = (raw as { data?: { endpoints?: unknown } } | null)?.data?.endpoints;
  if (!Array.isArray(endpoints)) {
    return [];
  }
  const companies = new Map<string, { headquarters?: unknown; datacenters?: unknown }>();
  const listed = (providers as { data?: unknown } | null)?.data;
  for (const entry of Array.isArray(listed) ? listed : []) {
    if (entry && typeof entry === 'object' && typeof (entry as { slug?: unknown }).slug === 'string') {
      companies.set((entry as { slug: string }).slug, entry as { headquarters?: unknown; datacenters?: unknown });
    }
  }
  const hosts = new Map<string, { host: OpenRouterHost; score: number; index: number }>();
  endpoints.forEach((entry: RawEndpoint, index) => {
    if (!entry || typeof entry !== 'object' || typeof entry.tag !== 'string' || !entry.tag || isFlex(entry)) {
      return;
    }
    const slug = hostSlug(entry.tag);
    const prices = endpointPrices(entry, at);
    const score = prices ? MIX.input * prices.prompt + MIX.cacheRead * (prices.cacheRead ?? prices.prompt) + MIX.output * prices.completion : Infinity;
    const up = !(typeof entry.status === 'number' && entry.status < 0) && !(typeof entry.uptime_last_30m === 'number' && entry.uptime_last_30m < 80);
    const quantization = typeof entry.quantization === 'string' && entry.quantization !== 'unknown' ? entry.quantization : null;
    const known = hosts.get(slug);
    if (known) {
      known.host.available ||= up;
      if (quantization && !known.host.quantizations.includes(quantization)) {
        known.host.quantizations.push(quantization);
      }
      if (prices && score < known.score) {
        known.host.rates = perMillion(prices);
        known.score = score;
      }
      return;
    }
    const company = companies.get(slug);
    const datacenters = Array.isArray(company?.datacenters) ? company.datacenters.filter((code): code is string => typeof code === 'string' && COUNTRY.test(code)) : [];
    hosts.set(slug, {
      host: {
        slug,
        name: typeof entry.provider_name === 'string' && entry.provider_name ? entry.provider_name.slice(0, 100) : slug,
        headquarters: typeof company?.headquarters === 'string' && COUNTRY.test(company.headquarters) ? company.headquarters : null,
        datacenters,
        rates: prices ? perMillion(prices) : null,
        quantizations: quantization ? [quantization] : [],
        available: up,
      },
      score,
      index,
    });
  });
  return [...hosts.values()]
    .sort((a, b) => Number(b.host.available) - Number(a.host.available) || a.score - b.score || a.index - b.index)
    .map((entry) => entry.host);
}

/** The provider of an endpoint tag (`deepinfra/fp8` → `deepinfra`), as `provider.order` takes it. */
function hostSlug(tag: string): string {
  return tag.split('/')[0] ?? tag;
}

/** Loads OpenRouter's endpoint list of a model; tests pass a stand-in. */
export type EndpointFetcher = (modelId: string) => Promise<unknown>;

const fetchEndpoints: EndpointFetcher = async (modelId) => {
  const response = await fetch(`https://openrouter.ai/api/v1/models/${modelId}/endpoints`, {
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
  });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
};

/** Loads OpenRouter's list of providers; tests pass a stand-in. */
export type ProviderFetcher = () => Promise<unknown>;

const fetchProviders: ProviderFetcher = async () => {
  const response = await fetch('https://openrouter.ai/api/v1/providers', { signal: AbortSignal.timeout(FETCH_TIMEOUT_MS) });
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}`);
  }
  return response.json();
};

/**
 * Which hosts a request for a model should try first. The endpoint lists
 * are public and kept for {@link ENDPOINTS_TTL_MS}; without one, OpenRouter
 * routes as it likes and nothing breaks.
 *
 * A conversation stays with the host that served its last call (see
 * {@link STICKY_MS}). Without that, a reply hopped between hosts (3.11.):
 * dozens of hosts list nearly the same price, so the order changed whenever
 * the list was reloaded and a host's uptime or price moved, and every call
 * OpenRouter sent elsewhere after a failure stayed there only for that call.
 */
export class OpenRouterRouting {
  readonly #lists = new Map<string, { loadedAt: number; raw: unknown }>();
  readonly #failed = new Map<string, number>();
  readonly #loading = new Map<string, Promise<unknown>>();
  readonly #hosts = new Map<string, { host: string; at: number }>();
  #providers: { loadedAt: number; raw: unknown } | null = null;
  #providersFailedAt = -RETRY_MS;
  #providersLoading: Promise<unknown> | null = null;

  /**
   * @param load - Loads an endpoint list.
   * @param now - The clock, for tests.
   * @param loadProviders - Loads the list of providers.
   */
  constructor(
    private readonly load: EndpointFetcher = fetchEndpoints,
    private readonly now: () => number = Date.now,
    private readonly loadProviders: ProviderFetcher = fetchProviders,
  ) {}

  /**
   * The hosts of a model for the user to pick from, see {@link hostChoices}.
   *
   * @param modelId - OpenRouter's model id.
   * @returns The hosts, or null if the model has no list or it could not be loaded.
   */
  async hosts(modelId: string): Promise<OpenRouterHost[] | null> {
    if (!ROUTABLE_MODEL.test(modelId)) {
      return null;
    }
    const [raw, providers] = await Promise.all([this.#list(modelId), this.#providerList()]);
    return raw === null ? null : hostChoices(raw, providers, new Date(this.now()));
  }

  /**
   * The cheapest hosts for a model right now, the conversation's host first
   * while it is still eligible.
   *
   * @param modelId - OpenRouter's model id.
   * @param options - See {@link rankEndpoints}.
   * @param conversation - Names the conversation for {@link stick}; without it, cheapest first.
   * @returns Tags for `provider.order`, or null if none are known.
   */
  async order(modelId: string, options: { noFourBit: boolean }, conversation?: string): Promise<string[] | null> {
    if (!ROUTABLE_MODEL.test(modelId)) {
      return null;
    }
    const raw = await this.#list(modelId);
    const now = this.now();
    const ranked = raw === null ? [] : rankHosts(raw, new Date(now), options);
    const sticky = conversation === undefined ? undefined : this.#hosts.get(stickyKey(modelId, conversation));
    const own = sticky && now - sticky.at < STICKY_MS ? ranked.findIndex((endpoint) => endpoint.host === sticky.host) : -1;
    if (own > 0) {
      ranked.unshift(...ranked.splice(own, 1));
    }
    const order = ranked.slice(0, MAX_ORDER).map((endpoint) => endpoint.tag);
    return order.length > 0 ? order : null;
  }

  /**
   * Remembers the host that served a conversation's call, so its next calls
   * go there first (see {@link order}).
   *
   * @param modelId - OpenRouter's model id.
   * @param conversation - Names the conversation.
   * @param host - The host as OpenRouter's answer names it (`provider`).
   */
  stick(modelId: string, conversation: string, host: string): void {
    const key = stickyKey(modelId, conversation);
    // Re-inserted, so the map's order is the order of last use.
    this.#hosts.delete(key);
    this.#hosts.set(key, { host, at: this.now() });
    for (const oldest of this.#hosts.keys()) {
      if (this.#hosts.size <= MAX_STICKY) {
        break;
      }
      this.#hosts.delete(oldest);
    }
  }

  /**
   * Loads a model's endpoint list ahead of its calls, so {@link rates} knows
   * its hosts when the bill comes in.
   *
   * @param modelId - OpenRouter's model id.
   */
  async preload(modelId: string): Promise<void> {
    if (ROUTABLE_MODEL.test(modelId)) {
      await this.#list(modelId);
    }
  }

  /**
   * The list prices of the host that ran a call, from the list loaded for
   * the model (see {@link preload}).
   *
   * @param modelId - OpenRouter's model id.
   * @param host - The host's name as OpenRouter's answer gives it.
   * @param at - When the call ran.
   * @returns US dollars per million tokens, or null if unknown.
   */
  rates(modelId: string, host: string, at: Date): TokenRates | null {
    const raw = this.#lists.get(modelId)?.raw;
    return raw === undefined ? null : hostRates(raw, host, at);
  }

  async #list(modelId: string): Promise<unknown> {
    const now = this.now();
    const cached = this.#lists.get(modelId);
    if (cached && now - cached.loadedAt < ENDPOINTS_TTL_MS) {
      return cached.raw;
    }
    if (now - (this.#failed.get(modelId) ?? -RETRY_MS) < RETRY_MS) {
      return cached?.raw ?? null;
    }
    let loading = this.#loading.get(modelId);
    if (!loading) {
      loading = this.load(modelId)
        .then((raw) => {
          this.#lists.set(modelId, { loadedAt: this.now(), raw });
          this.#failed.delete(modelId);
          return raw;
        })
        .catch((error: unknown) => {
          this.#failed.set(modelId, this.now());
          console.warn(`[openrouter] endpoints of ${modelId} not loaded:`, error instanceof Error ? error.message : error);
          return cached?.raw ?? null;
        })
        .finally(() => this.#loading.delete(modelId));
      this.#loading.set(modelId, loading);
    }
    return loading;
  }

  /** Where the providers sit; only for showing, so a failure means "unknown". */
  async #providerList(): Promise<unknown> {
    const now = this.now();
    if (this.#providers && now - this.#providers.loadedAt < PROVIDERS_TTL_MS) {
      return this.#providers.raw;
    }
    if (now - this.#providersFailedAt < RETRY_MS) {
      return this.#providers?.raw ?? null;
    }
    this.#providersLoading ??= this.loadProviders()
      .then((raw) => {
        this.#providers = { loadedAt: this.now(), raw };
        return raw;
      })
      .catch((error: unknown) => {
        this.#providersFailedAt = this.now();
        console.warn('[openrouter] providers not loaded:', error instanceof Error ? error.message : error);
        return this.#providers?.raw ?? null;
      })
      .finally(() => {
        this.#providersLoading = null;
      });
    return this.#providersLoading;
  }
}

/**
 * The `provider` field of a request with the cheapest hosts first. A
 * request that already says how to route (order, sort, only) keeps that.
 *
 * @param existing - The request's own `provider` field.
 * @param order - Hosts from {@link OpenRouterRouting.order}.
 * @returns The field to send, or undefined to leave it alone.
 */
export function cheapestProvider(existing: unknown, order: string[] | null): Record<string, unknown> | undefined {
  if (!order) {
    return undefined;
  }
  const current = existing && typeof existing === 'object' && !Array.isArray(existing) ? (existing as Record<string, unknown>) : {};
  if (current.order !== undefined || current.sort !== undefined || current.only !== undefined) {
    return undefined;
  }
  // A fixed first choice also keeps the prompt cache on one host.
  return { ...current, order, allow_fallbacks: true };
}

/**
 * The `provider` field of a request whose model the user pinned to one
 * host: that host only, never a fallback, because the user chose where the
 * conversation goes (not to a company in another country, say). A failing
 * host fails the request. The request's own routing gives way, so an agent
 * cannot route around the choice; other fields (`data_collection` …) stay.
 *
 * @param existing - The request's own `provider` field.
 * @param host - The provider's slug, e.g. `deepinfra`.
 * @returns The field to send.
 */
export function pinnedProvider(existing: unknown, host: string): Record<string, unknown> {
  const current = existing && typeof existing === 'object' && !Array.isArray(existing) ? { ...(existing as Record<string, unknown>) } : {};
  for (const field of ROUTING_FIELDS) {
    delete current[field];
  }
  return { ...current, order: [host], allow_fallbacks: false };
}

/**
 * The `prompt_cache_key` of a conversation. OpenAI-style hosts use it to
 * send the calls of a conversation to the server that holds its prompt
 * cache; DeepInfra documents it. Whether OpenRouter passes it on to the
 * host is not documented (6.11.2026); the cache share of the calls tells.
 * Hashed, so no ids of this server leave it.
 *
 * @param conversation - Names the conversation: a chat id, or an agent's key.
 * @returns The key.
 */
export function cacheKey(conversation: string): string {
  return `harness-${createHash('sha256').update(conversation).digest('hex').slice(0, 32)}`;
}

/**
 * Call options of an ordinary chat with OpenRouter's routing and cache key
 * in the request body; agents' requests get both in the pass-through.
 *
 * @param options - The call's options so far (thinking effort).
 * @param request - `provider` from {@link pinnedProvider} or {@link cheapestProvider}, and the cache key.
 * @returns The options with the fields in `providerOptions.openrouter`.
 */
export function withOpenRouter(options: EffortOptions, request: { provider?: Record<string, unknown>; cacheKey?: string }): EffortOptions {
  if (!request.provider && !request.cacheKey) {
    return options;
  }
  const openrouter = options.providerOptions?.openrouter ?? {};
  return {
    ...options,
    providerOptions: {
      ...options.providerOptions,
      openrouter: {
        ...openrouter,
        ...(request.provider ? { provider: request.provider as JSONValue } : {}),
        ...(request.cacheKey ? { prompt_cache_key: request.cacheKey } : {}),
      },
    },
  };
}

/** What OpenRouter billed for a call, with the tokens it counted. */
export interface BilledUsage {
  /** The bill. */
  cost: BilledCost;
  /** Prompt tokens, cached ones included. */
  promptTokens: number;
  /** Completion tokens, reasoning included. */
  completionTokens: number;
}

/**
 * Reads the bill from an answer's `usage`.
 *
 * @param usage - The `usage` object of a chunk or answer.
 * @param host - The answer's `provider`: the host OpenRouter routed to.
 * @returns The bill, or null if the answer has no cost.
 */
export function billedUsage(usage: unknown, host?: unknown): BilledUsage | null {
  const raw = (usage ?? {}) as {
    cost?: unknown;
    prompt_tokens?: unknown;
    completion_tokens?: unknown;
    cost_details?: Record<string, unknown> | null;
  };
  const total = price(raw.cost);
  const promptTokens = price(raw.prompt_tokens);
  const completionTokens = price(raw.completion_tokens);
  if (total === undefined || promptTokens === undefined || completionTokens === undefined) {
    return null;
  }
  const input = price(raw.cost_details?.upstream_inference_prompt_cost);
  const output = price(raw.cost_details?.upstream_inference_completions_cost);
  const named = typeof host === 'string' && host ? host.slice(0, 100) : undefined;
  return {
    cost: { total, ...(input !== undefined ? { input } : {}), ...(output !== undefined ? { output } : {}), ...(named ? { host: named } : {}) },
    promptTokens,
    completionTokens,
  };
}

/** A JSON answer is read for its bill only up to this size. */
const MAX_JSON_CHARS = 8 << 20;

/**
 * Passes a response body through unchanged and reports the bill of the
 * answer: in a stream it comes with the last chunk, in a JSON answer at
 * its end.
 *
 * @param body - The provider's response body.
 * @param stream - Whether it is an event stream.
 * @param onBill - Gets the bill once the body ended, if there was one.
 * @returns The body to send on.
 */
export function meterBill(body: ReadableStream<Uint8Array>, stream: boolean, onBill: (bill: BilledUsage) => void): ReadableStream<Uint8Array> {
  const decoder = new TextDecoder();
  let pending = '';
  let bill: BilledUsage | null = null;
  const readLine = (line: string) => {
    // Cheap test first: only the last chunk carries the usage.
    if (!line.startsWith('data:') || !line.includes('"usage"')) {
      return;
    }
    try {
      const chunk = JSON.parse(line.slice(5)) as { usage?: unknown; provider?: unknown };
      bill = billedUsage(chunk.usage, chunk.provider) ?? bill;
    } catch {
      // Not JSON (`[DONE]`, a broken line): nothing to read.
    }
  };
  return body.pipeThrough(
    new TransformStream<Uint8Array, Uint8Array>({
      transform(chunk, controller) {
        controller.enqueue(chunk);
        if (!stream && pending.length > MAX_JSON_CHARS) {
          return;
        }
        pending += decoder.decode(chunk, { stream: true });
        if (stream) {
          const lines = pending.split('\n');
          pending = lines.pop() ?? '';
          for (const line of lines) {
            readLine(line.trimEnd());
          }
        }
      },
      flush() {
        pending += decoder.decode();
        if (stream) {
          readLine(pending.trimEnd());
        } else if (pending.length <= MAX_JSON_CHARS) {
          try {
            const answer = JSON.parse(pending) as { usage?: unknown; provider?: unknown };
            bill = billedUsage(answer.usage, answer.provider);
          } catch {
            // An error page or no JSON at all.
          }
        }
        if (bill) {
          onBill(bill);
        }
      },
    }),
  );
}

/** Bills wait this long for the agent's report of their call. */
const BILL_TTL_MS = 15 * 60_000;

/** Bills kept per device at most. */
const MAX_BILLS = 200;

/**
 * Bills of agent calls that went through the pass-through, until the
 * agent's report of the same call picks them up. They are matched by the
 * tokens of the call, so replies running side by side on one device do not
 * swap their bills.
 */
export class AgentBills {
  readonly #bills = new Map<string, { bill: BilledUsage; at: number }[]>();

  /**
   * @param now - The clock, for tests.
   */
  constructor(private readonly now: () => number = Date.now) {}

  /**
   * Keeps a bill.
   *
   * @param deviceId - Device whose agent made the call.
   * @param bill - The bill.
   */
  record(deviceId: string, bill: BilledUsage): void {
    const now = this.now();
    const bills = (this.#bills.get(deviceId) ?? []).filter((entry) => now - entry.at < BILL_TTL_MS);
    bills.push({ bill, at: now });
    this.#bills.set(deviceId, bills.slice(-MAX_BILLS));
  }

  /**
   * Takes the bill of a call the agent reported.
   *
   * @param deviceId - Device of the agent.
   * @param input - Input tokens of the call, cached ones included.
   * @param output - Output tokens of the call.
   * @returns The bill, or null if none matches.
   */
  take(deviceId: string, input: number, output: number): BilledCost | null {
    const now = this.now();
    const bills = this.#bills.get(deviceId) ?? [];
    const index = bills.findIndex(
      (entry) => now - entry.at < BILL_TTL_MS && entry.bill.promptTokens === input && entry.bill.completionTokens === output,
    );
    if (index < 0) {
      return null;
    }
    const [found] = bills.splice(index, 1);
    return found?.bill.cost ?? null;
  }
}
