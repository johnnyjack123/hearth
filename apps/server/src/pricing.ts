import type { AgentModelInfo, MessageMetadata, ModelCallUsage, ProviderKind, TokenRates } from '@harness/shared';
import type { LanguageModelUsage, ProviderMetadata } from 'ai';

/**
 * Prices and limits of a model: US dollars per million tokens, as the
 * models.dev catalog lists them.
 */
export interface ModelPricing {
  /** Context window in tokens. */
  contextWindow?: number;
  /** Uncached input. */
  input?: number;
  /** Output, reasoning included. */
  output?: number;
  /** Input read from the provider's cache. */
  cacheRead?: number;
  /** Input written to the provider's cache. */
  cacheWrite?: number;
  /** Whether the model reads images; missing when the catalog does not say. */
  images?: boolean;
  /** Display name. */
  name?: string;
  /** Longest answer in tokens. */
  maxOutput?: number;
  /** Whether the model thinks before it answers. */
  reasoning?: boolean;
}

/** Open catalog of models with prices and context windows. */
const CATALOG_URL = 'https://models.dev/api.json';

/** The catalog is fetched again after a day. */
const CATALOG_TTL_MS = 24 * 60 * 60_000;

/** After a failed fetch the next try waits this long. */
const RETRY_MS = 10 * 60_000;

/** A chat waits at most this long for a catalog that is still loading. */
const LOOKUP_WAIT_MS = 3_000;

/** Catalog providers kept in memory; the full file has hundreds. */
const PROVIDERS = ['openai', 'anthropic', 'google', 'mistral', 'openrouter', 'deepseek'] as const;

/** A provider of the catalog harness keeps. */
export type CatalogProvider = (typeof PROVIDERS)[number];

/**
 * The catalog's provider for a credential, or null if it has none (local
 * models, unknown OpenAI-compatible endpoints).
 *
 * @param kind - Credential kind.
 * @param baseUrl - Custom endpoint of the credential, if any.
 * @returns The catalog provider id.
 */
export function catalogProvider(kind: ProviderKind, baseUrl: string | null): CatalogProvider | 'free' | null {
  const host = baseUrl ? (URL.parse(baseUrl)?.hostname ?? '') : '';
  switch (kind) {
    case 'openai':
      return !baseUrl || host === 'api.openai.com' ? 'openai' : null;
    case 'anthropic':
    case 'google':
    case 'mistral':
    case 'openrouter':
      return baseUrl ? null : kind;
    case 'openai-compatible':
      return host === 'api.deepseek.com' ? 'deepseek' : null;
    case 'ollama':
      // Runs on the user's own hardware.
      return 'free';
  }
}

type Catalog = Map<CatalogProvider, Map<string, ModelPricing>>;

interface RawModel {
  name?: unknown;
  reasoning?: unknown;
  limit?: { context?: unknown; output?: unknown };
  modalities?: { input?: unknown };
  cost?: { input?: unknown; output?: unknown; cache_read?: unknown; cache_write?: unknown };
}

function number(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined;
}

/**
 * Reduces the catalog file to the providers and fields harness uses.
 *
 * @param raw - Parsed `api.json` of models.dev.
 * @returns Prices per provider and model id.
 */
export function parseCatalog(raw: unknown): Catalog {
  const catalog: Catalog = new Map();
  if (!raw || typeof raw !== 'object') {
    return catalog;
  }
  for (const provider of PROVIDERS) {
    const models = (raw as Record<string, { models?: Record<string, RawModel> }>)[provider]?.models;
    if (!models || typeof models !== 'object') {
      continue;
    }
    const prices = new Map<string, ModelPricing>();
    for (const [id, model] of Object.entries(models)) {
      const pricing: ModelPricing = {};
      const context = number(model.limit?.context);
      const input = number(model.cost?.input);
      const output = number(model.cost?.output);
      const cacheRead = number(model.cost?.cache_read);
      const cacheWrite = number(model.cost?.cache_write);
      if (context !== undefined) {
        pricing.contextWindow = context;
      }
      if (input !== undefined) {
        pricing.input = input;
      }
      if (output !== undefined) {
        pricing.output = output;
      }
      if (cacheRead !== undefined) {
        pricing.cacheRead = cacheRead;
      }
      if (cacheWrite !== undefined) {
        pricing.cacheWrite = cacheWrite;
      }
      const inputs = model.modalities?.input;
      if (Array.isArray(inputs)) {
        pricing.images = inputs.includes('image');
      }
      const maxOutput = number(model.limit?.output);
      if (maxOutput !== undefined && maxOutput > 0) {
        pricing.maxOutput = maxOutput;
      }
      if (typeof model.name === 'string' && model.name.trim()) {
        pricing.name = model.name.trim().slice(0, 200);
      }
      if (typeof model.reasoning === 'boolean') {
        pricing.reasoning = model.reasoning;
      }
      prices.set(id.toLowerCase(), pricing);
    }
    catalog.set(provider, prices);
  }
  return catalog;
}

/**
 * The models.dev catalog, fetched in the background and kept for a day. A
 * lookup never fails: without the catalog, a reply simply has no price.
 */
export class PricingCatalog {
  #catalog: Catalog | null = null;
  #loadedAt = 0;
  #failedAt = 0;
  #loading: Promise<void> | null = null;

  /**
   * @param fetchCatalog - Loads the raw catalog; tests pass a fake.
   */
  constructor(
    private readonly fetchCatalog: () => Promise<unknown> = async () => {
      const response = await fetch(CATALOG_URL, { signal: AbortSignal.timeout(20_000) });
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      return response.json();
    },
  ) {}

  /** Starts loading unless the catalog is fresh or a recent try failed. */
  refresh(): Promise<void> {
    const now = Date.now();
    const fresh = this.#catalog && now - this.#loadedAt < CATALOG_TTL_MS;
    if (fresh || this.#loading || now - this.#failedAt < RETRY_MS) {
      return this.#loading ?? Promise.resolve();
    }
    this.#loading = this.fetchCatalog()
      .then((raw) => {
        this.#catalog = parseCatalog(raw);
        this.#loadedAt = Date.now();
      })
      .catch((error: unknown) => {
        this.#failedAt = Date.now();
        console.warn('[pricing] models.dev catalog not loaded:', error instanceof Error ? error.message : error);
      })
      .finally(() => {
        this.#loading = null;
      });
    return this.#loading;
  }

  /**
   * Prices of a model at a moment (DeepSeek's depend on the hour, see
   * {@link pricesAt}).
   *
   * @param kind - Credential kind.
   * @param baseUrl - Its custom endpoint, if any.
   * @param modelId - The provider's model id.
   * @param at - When the model is called.
   * @returns The pricing, or null if unknown.
   */
  async lookup(kind: ProviderKind, baseUrl: string | null, modelId: string, at: Date = new Date()): Promise<ModelPricing | null> {
    const provider = catalogProvider(kind, baseUrl);
    const entry = await this.entry(kind, baseUrl, modelId);
    return entry && provider && provider !== 'free' ? pricesAt(provider, entry, at) : entry;
  }

  /**
   * A model's catalog entry as listed, for prices that depend on the hour
   * of each call (agents); see {@link pricesAt}.
   *
   * @param kind - Credential kind.
   * @param baseUrl - Its custom endpoint, if any.
   * @param modelId - The provider's model id.
   * @returns The entry, or null if unknown.
   */
  async entry(kind: ProviderKind, baseUrl: string | null, modelId: string): Promise<ModelPricing | null> {
    const provider = catalogProvider(kind, baseUrl);
    if (provider === 'free') {
      return { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 };
    }
    if (!provider) {
      return null;
    }
    const loading = this.refresh();
    if (!this.#catalog) {
      await Promise.race([loading, new Promise((resolve) => setTimeout(resolve, LOOKUP_WAIT_MS))]);
    }
    return this.#catalog?.get(provider)?.get(modelId.toLowerCase()) ?? null;
  }
}

/**
 * Whether DeepSeek charges its peak prices at a moment: Monday to Friday,
 * 01:00–04:00 and 06:00–10:00 UTC (api-docs.deepseek.com, 1.11.2026;
 * OpenRouter's DeepSeek endpoint lists the same hours). Outside them, at
 * weekends and on Chinese holidays it charges half. Holidays are not known
 * here, so on those the estimate is too high during these hours.
 *
 * @param at - The moment.
 * @returns True in peak hours.
 */
export function deepSeekPeak(at: Date): boolean {
  const day = at.getUTCDay();
  if (day === 0 || day === 6) {
    return false;
  }
  const minute = at.getUTCHours() * 60 + at.getUTCMinutes();
  return (minute >= 60 && minute < 240) || (minute >= 360 && minute < 600);
}

/**
 * A model's prices at a moment. models.dev lists DeepSeek's off-peak prices
 * (1.11.2026: 0.15 $ input for `deepseek-flash`, DeepSeek's page says 0.15
 * off-peak and 0.30 peak), so they double in {@link deepSeekPeak} hours.
 * pi's catalog lists the peak prices instead, which made agent chats show
 * twice what DeepSeek billed in the evening and at weekends.
 *
 * @param provider - Catalog provider.
 * @param pricing - The catalog's prices.
 * @param at - The moment.
 * @returns The prices that apply then.
 */
export function pricesAt(provider: CatalogProvider, pricing: ModelPricing, at: Date): ModelPricing {
  if (provider !== 'deepseek' || !deepSeekPeak(at)) {
    return pricing;
  }
  const double = (price: number | undefined) => (price === undefined ? undefined : price * 2);
  const peak: ModelPricing = { ...pricing };
  for (const key of ['input', 'output', 'cacheRead', 'cacheWrite'] as const) {
    const price = double(pricing[key]);
    if (price !== undefined) {
      peak[key] = price;
    }
  }
  return peak;
}

/**
 * What a harness needs to use a model its own catalog lacks, from the
 * server's catalog entry.
 *
 * @param pricing - The entry, if the catalog has the model.
 * @returns The description for the device; empty without an entry.
 */
export function agentModelInfo(pricing: ModelPricing | null): AgentModelInfo {
  if (!pricing) {
    return {};
  }
  const info: AgentModelInfo = {};
  if (pricing.name) {
    info.name = pricing.name;
  }
  if (pricing.contextWindow) {
    info.contextWindow = pricing.contextWindow;
  }
  if (pricing.maxOutput) {
    info.maxOutput = pricing.maxOutput;
  }
  if (pricing.reasoning !== undefined) {
    info.reasoning = pricing.reasoning;
  }
  if (pricing.images !== undefined) {
    info.images = pricing.images;
  }
  if (pricing.input !== undefined && pricing.output !== undefined) {
    info.cost = {
      input: pricing.input,
      output: pricing.output,
      ...(pricing.cacheRead !== undefined ? { cacheRead: pricing.cacheRead } : {}),
      ...(pricing.cacheWrite !== undefined ? { cacheWrite: pricing.cacheWrite } : {}),
    };
  }
  return info;
}

/**
 * Tokens of one model call of an agent, as pi reports them.
 */
export interface AgentCallTokens {
  /** Input, cached tokens included. */
  input: number;
  /** Part of the input read from the cache. */
  cacheRead: number;
  /** Output, reasoning included. */
  output: number;
  /** What pi's own catalog says the call cost. */
  agentCost?: number;
}

/**
 * The cost of an agent's model call. pi prices calls from its own catalog,
 * which knows neither where OpenRouter sent a call nor DeepSeek's hours. So:
 * what the provider billed if the pass-through saw it (OpenRouter), DeepSeek
 * from the server's catalog at the time of the call, everything else as pi
 * says (pi knows cache writes, which reach the server only inside `input`).
 *
 * @param call - The call's tokens.
 * @param provider - Catalog provider of the chat's credential.
 * @param pricing - The server's prices at the time of the call, if known.
 * @param billed - What the provider billed, if known.
 * @returns The call's cost fields for `usage.calls`.
 */
export function agentCallCost(
  call: AgentCallTokens,
  provider: CatalogProvider | 'free' | null,
  pricing: ModelPricing | null,
  billed: BilledCost | null,
  hostRates?: HostRates,
): Pick<ModelCallUsage, 'cost' | 'billed' | 'host' | 'rates'> {
  if (billed) {
    return billedFields(billed, hostRates);
  }
  if ((provider === 'deepseek' || provider === 'free') && pricing?.input !== undefined && pricing.output !== undefined) {
    const usage: LanguageModelUsage = {
      inputTokens: call.input,
      inputTokenDetails: { noCacheTokens: Math.max(0, call.input - call.cacheRead), cacheReadTokens: call.cacheRead, cacheWriteTokens: 0 },
      outputTokens: call.output,
      outputTokenDetails: { textTokens: undefined, reasoningTokens: undefined },
      totalTokens: call.input + call.output,
    };
    const { cost, rates } = callUsage(usage, pricing, []);
    return cost ? { cost, ...(rates ? { rates } : {}) } : {};
  }
  return call.agentCost === undefined ? {} : { cost: { total: call.agentCost } };
}

/** What a provider billed for one model call, as OpenRouter reports it. */
export interface BilledCost {
  /** US dollars. */
  total: number;
  /** Part for the prompt, cached tokens included. */
  input?: number;
  /** Part for the completion, reasoning included. */
  output?: number;
  /** The host OpenRouter routed the call to, e.g. `DeepInfra`. */
  host?: string;
}

/** A host's list prices by the name OpenRouter gives it, for billed calls (`OpenRouterRouting.rates`). */
export type HostRates = (host: string) => TokenRates | null;

/**
 * The prices of a catalog entry, as `usage.rates` and `calls[].rates` keep them.
 *
 * @param pricing - The model's prices at the time of the call.
 * @returns US dollars per million tokens, or undefined without input and output prices.
 */
export function catalogRates(pricing: ModelPricing | null): TokenRates | undefined {
  if (pricing?.input === undefined || pricing.output === undefined) {
    return undefined;
  }
  return {
    input: pricing.input,
    output: pricing.output,
    ...(pricing.cacheRead !== undefined ? { cacheRead: pricing.cacheRead } : {}),
    ...(pricing.cacheWrite !== undefined ? { cacheWrite: pricing.cacheWrite } : {}),
  };
}

/**
 * A billed call's fields in `usage.calls`: the cost without the host, the
 * host, and its list prices if known.
 *
 * @param billed - The bill.
 * @param hostRates - Looks up the host's prices.
 * @returns `cost`, `billed`, `host` and `rates`.
 */
function billedFields(billed: BilledCost, hostRates: HostRates | undefined): Pick<ModelCallUsage, 'cost' | 'billed' | 'host' | 'rates'> {
  const { host, ...cost } = billed;
  const rates = host ? hostRates?.(host) : null;
  return { cost, billed: true, ...(host ? { host } : {}), ...(rates ? { rates } : {}) };
}

/**
 * Reads what OpenRouter billed for a call from its provider metadata (see
 * `openRouterCost` in `llm.ts`).
 *
 * @param metadata - Provider metadata of the call.
 * @returns The billed cost, or null for other providers.
 */
export function billedCost(metadata: ProviderMetadata | undefined): BilledCost | null {
  const entry = metadata?.openrouter as { cost?: unknown; promptCost?: unknown; completionCost?: unknown; host?: unknown } | undefined;
  const dollars = (value: unknown) => (typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined);
  const total = dollars(entry?.cost);
  if (total === undefined) {
    return null;
  }
  const input = dollars(entry?.promptCost);
  const output = dollars(entry?.completionCost);
  const host = typeof entry?.host === 'string' && entry.host ? entry.host : undefined;
  return { total, ...(input !== undefined ? { input } : {}), ...(output !== undefined ? { output } : {}), ...(host ? { host } : {}) };
}

/**
 * Usage of a reply for the chat: tokens, the context size after the last
 * model call (what the next message starts from) and the cost.
 *
 * @param total - Summed usage of all model calls of the reply.
 * @param lastStep - Usage of the last model call.
 * @param pricing - Prices of the model, if known.
 * @param calls - Each model call of the reply, see {@link callUsage}.
 * @returns The `usage` field of the message metadata.
 */
export function usageMetadata(
  total: LanguageModelUsage,
  lastStep: LanguageModelUsage | undefined,
  pricing: ModelPricing | null,
  calls: ModelCallUsage[] = [],
): NonNullable<MessageMetadata['usage']> {
  const input = total.inputTokens ?? 0;
  const output = total.outputTokens ?? 0;
  const cacheRead = total.inputTokenDetails.cacheReadTokens ?? 0;
  const cacheWrite = total.inputTokenDetails.cacheWriteTokens ?? 0;
  const uncached = total.inputTokenDetails.noCacheTokens ?? Math.max(0, input - cacheRead - cacheWrite);
  const last = lastStep ?? total;
  const usage: NonNullable<MessageMetadata['usage']> = {
    inputTokens: input,
    outputTokens: output,
    contextTokens: (last.inputTokens ?? 0) + (last.outputTokens ?? 0),
  };
  if (cacheRead > 0) {
    usage.cachedInputTokens = cacheRead;
  }
  if (pricing?.contextWindow !== undefined) {
    usage.contextWindow = pricing.contextWindow;
  }
  if (calls.length > 0 && calls.every((call) => call.billed && call.cost)) {
    // What the provider billed beats any catalog: OpenRouter routes to hosts
    // whose prices differ (21.10.2026: listed 0.024 $, billed 0.14 $ per
    // million input tokens of DeepSeek V4.1 Flash).
    usage.cost = calls.reduce((sum, call) => sum + (call.cost?.total ?? 0), 0);
    usage.billed = true;
  } else if (pricing?.input !== undefined && pricing.output !== undefined) {
    // Cache prices default to the input price when the catalog has none.
    usage.cost =
      (uncached * pricing.input +
        cacheRead * (pricing.cacheRead ?? pricing.input) +
        cacheWrite * (pricing.cacheWrite ?? pricing.input) +
        output * pricing.output) /
      1_000_000;
    usage.rates = catalogRates(pricing);
  }
  const reasoning = total.outputTokenDetails.reasoningTokens;
  if (reasoning) {
    usage.reasoningTokens = reasoning;
  }
  if (calls.length > 0) {
    usage.calls = calls;
  }
  return usage;
}

/**
 * The usage of two model calls together, for the running total of a reply.
 *
 * @param sum - The calls so far; undefined before the first.
 * @param call - One more call.
 * @returns Both summed.
 */
export function addUsage(sum: LanguageModelUsage | undefined, call: LanguageModelUsage): LanguageModelUsage {
  if (!sum) {
    return call;
  }
  const add = (a: number | undefined, b: number | undefined) => (a === undefined && b === undefined ? undefined : (a ?? 0) + (b ?? 0));
  return {
    inputTokens: add(sum.inputTokens, call.inputTokens),
    inputTokenDetails: {
      noCacheTokens: add(sum.inputTokenDetails.noCacheTokens, call.inputTokenDetails.noCacheTokens),
      cacheReadTokens: add(sum.inputTokenDetails.cacheReadTokens, call.inputTokenDetails.cacheReadTokens),
      cacheWriteTokens: add(sum.inputTokenDetails.cacheWriteTokens, call.inputTokenDetails.cacheWriteTokens),
    },
    outputTokens: add(sum.outputTokens, call.outputTokens),
    outputTokenDetails: {
      textTokens: add(sum.outputTokenDetails.textTokens, call.outputTokenDetails.textTokens),
      reasoningTokens: add(sum.outputTokenDetails.reasoningTokens, call.outputTokenDetails.reasoningTokens),
    },
    totalTokens: add(sum.totalTokens, call.totalTokens),
  };
}

/**
 * Adds calls of helper models (Jev, embeddings, the title) to a reply's
 * usage; their cost joins the reply's cost when that is known.
 *
 * @param usage - The reply's usage.
 * @param helpers - The calls to add.
 * @returns The usage with the calls.
 */
export function withHelpers(usage: NonNullable<MessageMetadata['usage']>, helpers: ModelCallUsage[]): NonNullable<MessageMetadata['usage']> {
  if (helpers.length === 0) {
    return usage;
  }
  const extra = helpers.reduce((sum, call) => sum + (call.cost?.total ?? 0), 0);
  return {
    ...usage,
    helpers: [...(usage.helpers ?? []), ...helpers],
    ...(usage.cost !== undefined ? { cost: usage.cost + extra } : {}),
  };
}

/**
 * Tokens and cost of one model call, for the usage details below the reply.
 *
 * @param step - Usage of the call.
 * @param pricing - Prices of the model, if known.
 * @param tools - Tools the call asked for.
 * @param billed - What the provider billed, if it says; replaces the estimate.
 * @param hostRates - The list prices of the host that ran a billed call.
 * @returns The call's entry in `usage.calls`.
 */
export function callUsage(
  step: LanguageModelUsage,
  pricing: ModelPricing | null,
  tools: string[],
  billed: BilledCost | null = null,
  hostRates?: HostRates,
): ModelCallUsage {
  const input = step.inputTokens ?? 0;
  const cacheRead = step.inputTokenDetails.cacheReadTokens ?? 0;
  const cacheWrite = step.inputTokenDetails.cacheWriteTokens ?? 0;
  const output = step.outputTokens ?? 0;
  const call: ModelCallUsage = { input, cacheRead, cacheWrite, output, tools };
  const reasoning = step.outputTokenDetails.reasoningTokens;
  if (reasoning !== undefined) {
    call.reasoning = reasoning;
  }
  if (billed) {
    Object.assign(call, billedFields(billed, hostRates));
  } else if (pricing?.input !== undefined && pricing.output !== undefined) {
    const uncached = step.inputTokenDetails.noCacheTokens ?? Math.max(0, input - cacheRead - cacheWrite);
    const cost = {
      input: (uncached * pricing.input) / 1_000_000,
      cacheRead: (cacheRead * (pricing.cacheRead ?? pricing.input)) / 1_000_000,
      cacheWrite: (cacheWrite * (pricing.cacheWrite ?? pricing.input)) / 1_000_000,
      output: (output * pricing.output) / 1_000_000,
    };
    call.cost = { total: cost.input + cost.cacheRead + cost.cacheWrite + cost.output, ...cost };
    call.rates = catalogRates(pricing);
  }
  return call;
}
