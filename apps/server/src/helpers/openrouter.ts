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
