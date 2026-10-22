import { createAnthropic } from '@ai-sdk/anthropic';
import { createGoogle } from '@ai-sdk/google';
import { createMistral } from '@ai-sdk/mistral';
import { createOpenAI } from '@ai-sdk/openai';
import { createOpenAICompatible, type MetadataExtractor } from '@ai-sdk/openai-compatible';
import { type ModelInfo, PROVIDERS, type ProviderKind } from '@harness/shared';
import type { LanguageModel, ProviderMetadata } from 'ai';

/**
 * A credential with its decrypted API key, ready to call the provider.
 */
export interface ResolvedCredential {
  /** Credential id, used as cache key. */
  id: string;
  /** Provider kind. */
  kind: ProviderKind;
  /** Custom endpoint, or null for the provider default. */
  baseUrl: string | null;
  /** Decrypted API key, or null if none is stored. */
  apiKey: string | null;
}

/**
 * Talks to LLM providers. Injected into the app so tests can replace it with
 * mock models.
 */
export interface LlmGateway {
  /**
   * Builds a language model for one request.
   *
   * @param credential - Credential to authenticate with.
   * @param modelId - Provider-specific model id.
   * @returns A model usable with `streamText`/`generateText`.
   */
  createModel(credential: ResolvedCredential, modelId: string): LanguageModel;
  /**
   * Lists the models a credential can use.
   *
   * @param credential - Credential to query with.
   * @returns Models sorted by display name.
   * @throws {ProviderRequestError} If the provider rejects or cannot be reached.
   */
  listModels(credential: ResolvedCredential): Promise<ModelInfo[]>;
  /**
   * Drops cached data for a credential after it was changed or deleted.
   *
   * @param credentialId - Affected credential.
   */
  invalidate(credentialId: string): void;
}

/**
 * Raised when a provider's model list cannot be fetched. The message is safe to
 * show to the credential's owner.
 */
export class ProviderRequestError extends Error {
  /**
   * @param message - Description including the HTTP status if there was one.
   */
  constructor(message: string) {
    super(message);
    this.name = 'ProviderRequestError';
  }
}

const LIST_TIMEOUT_MS = 10_000;

// OpenAI's /models also returns embedding, audio, image and moderation models,
// which cannot be used for chat.
const OPENAI_NON_CHAT = /(embedding|whisper|tts|dall-e|davinci|babbage|moderation|transcribe|realtime|audio|image|search|computer-use|sora)/i;

function endpoint(credential: ResolvedCredential): string {
  const url = credential.baseUrl ?? PROVIDERS[credential.kind].defaultBaseUrl;
  if (!url) {
    throw new ProviderRequestError('No endpoint configured for this credential');
  }
  return url.replace(/\/+$/, '');
}

async function getJson(url: string, headers: Record<string, string>): Promise<unknown> {
  let response: Response;
  try {
    response = await fetch(url, { headers, signal: AbortSignal.timeout(LIST_TIMEOUT_MS), redirect: 'error' });
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    throw new ProviderRequestError(`Provider not reachable: ${reason}`);
  }
  if (!response.ok) {
    const detail = (await response.text().catch(() => '')).slice(0, 300);
    throw new ProviderRequestError(`Provider responded with HTTP ${response.status}${detail ? `: ${detail}` : ''}`);
  }
  return response.json();
}

function bearer(apiKey: string | null): Record<string, string> {
  return apiKey ? { authorization: `Bearer ${apiKey}` } : {};
}

interface IdList {
  data?: { id?: unknown; name?: unknown; display_name?: unknown; capabilities?: { completion_chat?: unknown } }[];
}

function fromIdList(body: unknown, filter: (entry: NonNullable<IdList['data']>[number]) => boolean = () => true): ModelInfo[] {
  const entries = (body as IdList).data ?? [];
  return entries
    .filter((entry) => typeof entry.id === 'string' && filter(entry))
    .map((entry) => {
      const id = entry.id as string;
      const name = typeof entry.display_name === 'string' ? entry.display_name : typeof entry.name === 'string' ? entry.name : id;
      return { id, name };
    });
}

async function fetchModels(credential: ResolvedCredential): Promise<ModelInfo[]> {
  const base = endpoint(credential);
  switch (credential.kind) {
    case 'anthropic': {
      const body = await getJson(`${base}/models?limit=1000`, {
        'x-api-key': credential.apiKey ?? '',
        'anthropic-version': '2023-06-01',
      });
      return fromIdList(body);
    }
    case 'openai': {
      const body = await getJson(`${base}/models`, bearer(credential.apiKey));
      return fromIdList(body, (entry) => !OPENAI_NON_CHAT.test(String(entry.id)));
    }
    case 'google': {
      const body = (await getJson(`${base}/models?pageSize=1000`, {
        'x-goog-api-key': credential.apiKey ?? '',
      })) as { models?: { name?: string; displayName?: string; supportedGenerationMethods?: string[] }[] };
      return (body.models ?? [])
        .filter((model) => model.name && model.supportedGenerationMethods?.includes('generateContent'))
        .map((model) => {
          const id = (model.name as string).replace(/^models\//, '');
          return { id, name: model.displayName ?? id };
        });
    }
    case 'mistral': {
      const body = await getJson(`${base}/models`, bearer(credential.apiKey));
      return fromIdList(body, (entry) => entry.capabilities?.completion_chat !== false);
    }
    case 'openrouter':
    case 'ollama':
    case 'openai-compatible': {
      const body = await getJson(`${base}/models`, bearer(credential.apiKey));
      return fromIdList(body);
    }
  }
}

/** OpenRouter's billed cost from the `usage` of an answer, as provider metadata `openrouter`. */
function costOf(usage: unknown): ProviderMetadata | undefined {
  const { cost, cost_details: details } = (usage ?? {}) as { cost?: unknown; cost_details?: Record<string, unknown> };
  if (typeof cost !== 'number') {
    return undefined;
  }
  const part = (value: unknown) => (typeof value === 'number' ? value : null);
  return {
    openrouter: {
      cost,
      promptCost: part(details?.upstream_inference_prompt_cost),
      completionCost: part(details?.upstream_inference_completions_cost),
    },
  };
}

/**
 * OpenRouter reports in every answer what it billed. Catalog prices can be
 * far off, because OpenRouter routes a model to hosts with their own prices
 * (21.10.2026: DeepSeek V4.1 Flash listed at 0.024 $, billed by DeepInfra at
 * 0.14 $ per million input tokens). `pricing.billedCost` reads it back.
 */
const openRouterCost: MetadataExtractor = {
  extractMetadata: async ({ parsedBody }) => costOf((parsedBody as { usage?: unknown } | undefined)?.usage),
  createStreamExtractor: () => {
    let usage: unknown;
    return {
      processChunk(chunk) {
        // The usage comes with the last chunk.
        const next = (chunk as { usage?: unknown } | undefined)?.usage;
        if (next) {
          usage = next;
        }
      },
      buildMetadata: () => costOf(usage),
    };
  },
};

/**
 * Creates the production {@link LlmGateway} backed by the AI SDK providers.
 * Model lists are cached per credential for a few minutes, because the model
 * picker requests them on every page load.
 *
 * @returns The gateway.
 */
export function createLlmGateway(): LlmGateway {
  const cache = new Map<string, { expires: number; models: ModelInfo[] }>();
  const ttlMs = 5 * 60_000;

  return {
    createModel(credential, modelId) {
      const apiKey = credential.apiKey ?? undefined;
      const baseURL = endpoint(credential);
      switch (credential.kind) {
        case 'anthropic':
          return createAnthropic({ apiKey, baseURL })(modelId);
        case 'openai':
          return createOpenAI({ apiKey, baseURL })(modelId);
        case 'google':
          return createGoogle({ apiKey, baseURL })(modelId);
        case 'mistral':
          return createMistral({ apiKey, baseURL })(modelId);
        case 'openrouter':
          return createOpenAICompatible({ name: credential.kind, baseURL, apiKey, includeUsage: true, metadataExtractor: openRouterCost })(modelId);
        case 'ollama':
        case 'openai-compatible':
          return createOpenAICompatible({ name: credential.kind, baseURL, apiKey })(modelId);
      }
    },
    async listModels(credential) {
      const cached = cache.get(credential.id);
      if (cached && cached.expires > Date.now()) {
        return cached.models;
      }
      const models = (await fetchModels(credential)).sort((a, b) => a.name.localeCompare(b.name));
      cache.set(credential.id, { expires: Date.now() + ttlMs, models });
      return models;
    },
    invalidate(credentialId) {
      cache.delete(credentialId);
    },
  };
}
