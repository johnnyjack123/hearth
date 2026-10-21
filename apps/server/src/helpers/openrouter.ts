/**
 * Helper models of research mode, reached through OpenRouter with the key of
 * the user who chats: Jev ranks search hits, an embedding model helps pick
 * the sections of a page. A failing helper never breaks a reply; callers
 * fall back to what they had.
 */
import type { ModelCallUsage } from '@harness/shared';

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
  /** For the developer view. */
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

/**
 * Embeds texts with the embedding model, all in one call.
 *
 * @param helpers - See {@link Helpers}.
 * @param inputs - Texts; the first is usually the query.
 * @param signal - Ends the call with the chat request.
 * @returns One vector per input in the same order, or null if the call failed.
 */
export async function embed(helpers: Helpers, inputs: string[], signal?: AbortSignal): Promise<number[][] | null> {
  if (inputs.length === 0) {
    return [];
  }
  try {
    const body = await post(
      helpers,
      new URL('embeddings', `${helpers.apiUrl.replace(/\/+$/, '')}/`).href,
      { model: helpers.embeddingModel, input: inputs },
      signal,
    );
    const usage = (body.usage ?? {}) as { prompt_tokens?: unknown; total_tokens?: unknown; cost?: unknown };
    const tokens = typeof usage.prompt_tokens === 'number' ? usage.prompt_tokens : typeof usage.total_tokens === 'number' ? usage.total_tokens : 0;
    let cost = typeof usage.cost === 'number' ? usage.cost : null;
    if (cost === null) {
      const price = await embeddingPrice(helpers, signal);
      cost = price === null ? null : tokens * price;
    }
    record(helpers, helpers.embeddingModel, 'sections', { input_tokens: tokens, output_tokens: 0, ...(cost === null ? {} : { cost }) });
    const vectors: number[][] = [];
    for (const item of Array.isArray(body.data) ? (body.data as { index?: unknown; embedding?: unknown }[]) : []) {
      if (typeof item.index === 'number' && Array.isArray(item.embedding)) {
        vectors[item.index] = item.embedding.filter((value): value is number => typeof value === 'number');
      }
    }
    return vectors.length === inputs.length && vectors.every(Boolean) ? vectors : null;
  } catch (error) {
    if (!signal?.aborted) {
      console.warn(`[helpers] ${helpers.embeddingModel} failed: ${error instanceof Error ? error.message : String(error)}`);
    }
    return null;
  }
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
