/**
 * Helper models of research mode, reached through OpenRouter with the key of
 * the user who chats: Jev ranks search hits, an embedding model helps pick
 * the sections of a page. A failing helper never breaks a reply; callers
 * fall back to what they had.
 */
import type { ModelCallUsage } from '@hearth/shared';

/** What a request may use; null when the user has no OpenRouter key or an admin switched helpers off. */
export interface Helpers {
  /** The user's OpenRouter key. */
  key: string;
  /** OpenRouter's API root, `https://openrouter.ai/api/v1` unless the credential names another. */
  apiUrl: string;
  /** Jev model, see `HelperSettings`. */
  jevModel: string;
  /** Embedding model, see `HelperSettings`. */
  embeddingModel: string;
  /** Time limit per call. */
  timeoutMs: number;
  /** Collects every call for the reply's usage. */
  usage: ModelCallUsage[];
}

/** Jev should answer quickly; a slow answer is worth less than none. */
export const HELPER_TIMEOUT_MS = 10_000;

/** One ordered scale for Jev, worst level first. */
export interface ScoreRequest {
  /** What Jev reads: the question and the items, as text or JSON. */
  state: unknown;
  /** One instruction per item, e.g. "How useful is result 3 …". */
  instructions: string[];
  /** The levels of the scale, worst first. */
  criteria: string[];
  /** For the usage details below the reply. */
  purpose: NonNullable<ModelCallUsage['purpose']>;
}

function record(helpers: Helpers, model: string, purpose: ModelCallUsage['purpose'], usage: unknown): void {
  const { input_tokens: input, output_tokens: output, cost } = (usage ?? {}) as Record<string, unknown>;
  helpers.usage.push({
    input: typeof input === 'number' ? input : 0,
    cacheRead: 0,
    cacheWrite: 0,
    output: typeof output === 'number' ? output : 0,
    tools: [],
    model,
    ...(purpose ? { purpose } : {}),
    ...(typeof cost === 'number' && Number.isFinite(cost) && cost >= 0 ? { cost: { total: cost } } : {}),
  });
}

async function post(helpers: Helpers, url: string, body: unknown, signal: AbortSignal | undefined): Promise<Record<string, unknown>> {
  const deadline = AbortSignal.timeout(helpers.timeoutMs);
  const response = await fetch(url, {
    method: 'POST',
    headers: { authorization: `Bearer ${helpers.key}`, 'content-type': 'application/json', accept: 'application/json' },
    body: JSON.stringify(body),
    redirect: 'error',
    signal: signal ? AbortSignal.any([signal, deadline]) : deadline,
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`HTTP ${response.status}: ${text.slice(0, 200)}`);
  }
  return JSON.parse(text) as Record<string, unknown>;
}

/** Prices of embedding models per token, from OpenRouter's model list; refreshed daily. */
const embeddingPrices = new Map<string, { price: number | null; at: number }>();
const PRICE_TTL_MS = 24 * 60 * 60_000;

async function embeddingPrice(helpers: Helpers, signal: AbortSignal | undefined): Promise<number | null> {
  const cached = embeddingPrices.get(helpers.embeddingModel);
  if (cached && Date.now() - cached.at < PRICE_TTL_MS) {
    return cached.price;
  }
  let price: number | null = null;
  try {
    const deadline = AbortSignal.timeout(helpers.timeoutMs);
    const response = await fetch(new URL('models?output_modalities=embeddings', `${helpers.apiUrl.replace(/\/+$/, '')}/`), {
      headers: { accept: 'application/json' },
      redirect: 'error',
      signal: signal ? AbortSignal.any([signal, deadline]) : deadline,
    });
    const list = response.ok ? ((await response.json()) as { data?: { id?: unknown; pricing?: { prompt?: unknown } }[] }) : null;
    const prompt = Number(list?.data?.find((model) => model.id === helpers.embeddingModel)?.pricing?.prompt);
    price = Number.isFinite(prompt) && prompt >= 0 ? prompt : null;
  } catch {
    price = null;
  }
  embeddingPrices.set(helpers.embeddingModel, { price, at: Date.now() });
  return price;
}

/** Texts per embedding call; the calls of one page run side by side. */
const EMBED_BATCH = 32;

/**
 * Models that can return shorter vectors (Matryoshka). 1,024 of Qwen3's
 * 4,096 dimensions barely change the ranking, but the answer shrinks to a
 * quarter: for a Wikipedia article (167 sections) from 14.7 MB to 3.6 MB,
 * from 8–15 s to 3–4 s (measured on 21.10.2026).
 */
const SHORTER_VECTORS = /^(qwen\/qwen3-embedding|openai\/text-embedding-3)/;

/**
 * Reads a vector sent as JSON numbers or as base64 of little-endian float32.
 *
 * @param value - The embedding as the API (or `fileVectors`) has it.
 * @returns The vector, or null if it is none.
 */
export function vectorOf(value: unknown): Float32Array | null {
  if (typeof value === 'string') {
    const bytes = Buffer.from(value, 'base64');
    if (bytes.length === 0 || bytes.length % 4 !== 0) {
      return null;
    }
    const vector = new Float32Array(bytes.length / 4);
    for (let index = 0; index < vector.length; index += 1) {
      vector[index] = bytes.readFloatLE(index * 4);
    }
    return vector;
  }
  return Array.isArray(value) && value.length > 0 && value.every((entry) => typeof entry === 'number') ? Float32Array.from(value as number[]) : null;
}

/** One embedding call; null if it failed. */
async function embedBatch(
  helpers: Helpers,
  inputs: string[],
  signal: AbortSignal | undefined,
): Promise<{ vectors: (Float32Array | null)[]; tokens: number; cost: number | null } | null> {
  try {
    const body = await post(
      helpers,
      new URL('embeddings', `${helpers.apiUrl.replace(/\/+$/, '')}/`).href,
      {
        model: helpers.embeddingModel,
        input: inputs,
        // Base64 is a quarter of the size of numbers written out as JSON.
        encoding_format: 'base64',
        ...(SHORTER_VECTORS.test(helpers.embeddingModel) ? { dimensions: 1024 } : {}),
      },
      signal,
    );
    const usage = (body.usage ?? {}) as { prompt_tokens?: unknown; total_tokens?: unknown; cost?: unknown };
    const tokens = typeof usage.prompt_tokens === 'number' ? usage.prompt_tokens : typeof usage.total_tokens === 'number' ? usage.total_tokens : 0;
    const vectors: (Float32Array | null)[] = inputs.map(() => null);
    for (const item of Array.isArray(body.data) ? (body.data as { index?: unknown; embedding?: unknown }[]) : []) {
      if (typeof item.index === 'number' && item.index >= 0 && item.index < inputs.length) {
        vectors[item.index] = vectorOf(item.embedding);
      }
    }
    return { vectors, tokens, cost: typeof usage.cost === 'number' ? usage.cost : null };
  } catch (error) {
    if (!signal?.aborted) {
      console.warn(`[helpers] ${helpers.embeddingModel} failed: ${error instanceof Error ? error.message : String(error)}`);
    }
    return null;
  }
}

/**
 * Embeds texts with the embedding model, in batches that run side by side.
 * A failed batch leaves its texts without a vector; the rest still counts.
 *
 * @param helpers - See {@link Helpers}.
 * @param inputs - Texts; the first is usually the query.
 * @param signal - Ends the calls with the chat request.
 * @returns One vector (or null) per input in the same order, or null if
 *   every batch failed.
 */
export async function embed(helpers: Helpers, inputs: string[], signal?: AbortSignal): Promise<(Float32Array | null)[] | null> {
  if (inputs.length === 0) {
    return [];
  }
  const batches: string[][] = [];
  for (let start = 0; start < inputs.length; start += EMBED_BATCH) {
    batches.push(inputs.slice(start, start + EMBED_BATCH));
  }
  const results = await Promise.all(batches.map((batch) => embedBatch(helpers, batch, signal)));
  if (results.every((result) => result === null)) {
    return null;
  }
  // One entry in the usage details, whatever the number of batches.
  const tokens = results.reduce((sum, result) => sum + (result?.tokens ?? 0), 0);
  let cost: number | null = results.every((result) => result === null || result.cost !== null)
    ? results.reduce((sum, result) => sum + (result?.cost ?? 0), 0)
    : null;
  if (cost === null) {
    const price = await embeddingPrice(helpers, signal);
    cost = price === null ? null : tokens * price;
  }
  record(helpers, helpers.embeddingModel, 'sections', { input_tokens: tokens, output_tokens: 0, ...(cost === null ? {} : { cost }) });
  return results.flatMap((result, index) => result?.vectors ?? (batches[index] ?? []).map(() => null));
}

/**
 * Asks Jev to place each item on a scale, all in one call.
 *
 * @param helpers - See {@link Helpers}.
 * @param request - See {@link ScoreRequest}.
 * @param signal - Ends the call with the chat request.
 * @returns One score per item from 0 (worst level) to 1 (best), or null if
 *   Jev could not be asked or its answer was incomplete.
 */
export async function jevScores(helpers: Helpers, request: ScoreRequest, signal?: AbortSignal): Promise<number[] | null> {
  if (request.instructions.length === 0) {
    return [];
  }
  const questions = Object.fromEntries(
    request.instructions.map((instructions, index) => [`item_${index + 1}`, { type: 'score', instructions, criteria: request.criteria }]),
  );
  try {
    const body = await post(
      helpers,
      new URL('/api/alpha/decisions', helpers.apiUrl).href,
      { model: helpers.jevModel, state: request.state, questions },
      signal,
    );
    record(helpers, helpers.jevModel, request.purpose, body.usage);
    const answers = (body.answers ?? {}) as Record<string, { score?: unknown } | undefined>;
    const top = Math.max(1, request.criteria.length - 1);
    const scores = request.instructions.map((_, index) => {
      const score = answers[`item_${index + 1}`]?.score;
      return typeof score === 'number' && Number.isFinite(score) ? Math.min(1, Math.max(0, score / top)) : null;
    });
    return scores.every((score) => score !== null) ? (scores as number[]) : null;
  } catch (error) {
    if (!signal?.aborted) {
      console.warn(`[helpers] ${helpers.jevModel} failed: ${error instanceof Error ? error.message : String(error)}`);
    }
    return null;
  }
}
