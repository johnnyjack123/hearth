/**
 * Thinking effort of a chat request, turned into what each provider
 * understands. The AI SDK's portable `reasoning` option covers most of
 * them; two endpoints need their own field. Nothing here touches the
 * prompt, so the provider's cache does not depend on the level.
 */
import type { EffortLevel, ProviderKind } from '@hearth/shared';
import type { streamText } from 'ai';

type CallOptions = Parameters<typeof streamText>[0];

/** What a model call gets for an effort level; spread into `streamText`. */
export interface EffortOptions {
  /** The AI SDK's portable level. */
  reasoning?: CallOptions['reasoning'];
  /** Provider-specific fields where the portable level does not fit. */
  providerOptions?: NonNullable<CallOptions['providerOptions']>;
}

/** DeepSeek's own API, as opposed to other OpenAI-compatible endpoints. */
function isDeepSeek(baseUrl: string | null): boolean {
  try {
    const host = new URL(baseUrl ?? '').hostname;
    return host === 'deepseek.com' || host.endsWith('.deepseek.com');
  } catch {
    return false;
  }
}

/**
 * The call options for an effort level.
 *
 * - Anthropic, OpenAI, Google, Mistral: the portable `reasoning` level,
 *   which their adapters map to effort or a token budget (`off` → `none`).
 * - OpenRouter: `reasoning: { effort }` in the body, the field it
 *   documents; the portable level would send `reasoning_effort`.
 * - Other OpenAI-compatible endpoints and Ollama: `reasoning_effort`.
 *   DeepSeek maps the levels to its own low, high and max and knows no
 *   `none`; there `off` switches thinking off with `thinking.type`.
 *
 * @param kind - Provider of the chat's credential.
 * @param baseUrl - Its endpoint, if it has its own.
 * @param effort - The level; missing for the provider's default.
 * @returns Options to spread into the model call; empty for the default.
 */
export function effortOptions(kind: ProviderKind, baseUrl: string | null, effort: EffortLevel | undefined): EffortOptions {
  if (!effort) {
    return {};
  }
  const level = effort === 'off' ? 'none' : effort;
  switch (kind) {
    case 'openrouter':
      return { providerOptions: { openrouter: { reasoning: { effort: level } } } };
    case 'openai-compatible':
    case 'ollama':
      if (effort === 'off' && isDeepSeek(baseUrl)) {
        return { providerOptions: { [kind]: { thinking: { type: 'disabled' } } } };
      }
      return { reasoning: level };
    default:
      return { reasoning: level };
  }
}
