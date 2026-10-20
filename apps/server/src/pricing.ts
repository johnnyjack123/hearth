import type { MessageMetadata, ModelCallUsage, ProviderKind } from '@harness/shared';
import type { LanguageModelUsage } from 'ai';

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

type CatalogProvider = (typeof PROVIDERS)[number];

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
  limit?: { context?: unknown };
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
   * Prices of a model.
   *
   * @param kind - Credential kind.
   * @param baseUrl - Its custom endpoint, if any.
   * @param modelId - The provider's model id.
   * @returns The pricing, or null if unknown.
   */
  async lookup(kind: ProviderKind, baseUrl: string | null, modelId: string): Promise<ModelPricing | null> {
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
  if (pricing?.input !== undefined && pricing.output !== undefined) {
    // Cache prices default to the input price when the catalog has none.
    usage.cost =
      (uncached * pricing.input +
        cacheRead * (pricing.cacheRead ?? pricing.input) +
        cacheWrite * (pricing.cacheWrite ?? pricing.input) +
        output * pricing.output) /
      1_000_000;
    usage.rates = {
      input: pricing.input,
      output: pricing.output,
      ...(pricing.cacheRead !== undefined ? { cacheRead: pricing.cacheRead } : {}),
      ...(pricing.cacheWrite !== undefined ? { cacheWrite: pricing.cacheWrite } : {}),
    };
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
 * Tokens and cost of one model call, for the developer view.
 *
 * @param step - Usage of the call.
 * @param pricing - Prices of the model, if known.
 * @param tools - Tools the call asked for.
 * @returns The call's entry in `usage.calls`.
 */
export function callUsage(step: LanguageModelUsage, pricing: ModelPricing | null, tools: string[]): ModelCallUsage {
  const input = step.inputTokens ?? 0;
  const cacheRead = step.inputTokenDetails.cacheReadTokens ?? 0;
  const cacheWrite = step.inputTokenDetails.cacheWriteTokens ?? 0;
  const output = step.outputTokens ?? 0;
  const call: ModelCallUsage = { input, cacheRead, cacheWrite, output, tools };
  const reasoning = step.outputTokenDetails.reasoningTokens;
  if (reasoning !== undefined) {
    call.reasoning = reasoning;
  }
  if (pricing?.input !== undefined && pricing.output !== undefined) {
    const uncached = step.inputTokenDetails.noCacheTokens ?? Math.max(0, input - cacheRead - cacheWrite);
    const cost = {
      input: (uncached * pricing.input) / 1_000_000,
      cacheRead: (cacheRead * (pricing.cacheRead ?? pricing.input)) / 1_000_000,
      cacheWrite: (cacheWrite * (pricing.cacheWrite ?? pricing.input)) / 1_000_000,
      output: (output * pricing.output) / 1_000_000,
    };
    call.cost = { total: cost.input + cost.cacheRead + cost.cacheWrite + cost.output, ...cost };
  }
  return call;
}
