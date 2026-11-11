/**
 * Identifiers of the LLM providers a user can store credentials for.
 */
export const PROVIDER_KINDS = [
  'anthropic',
  'openai',
  'google',
  'mistral',
  'openrouter',
  'ollama',
  'openai-compatible',
] as const;

/**
 * One of the supported LLM provider identifiers.
 */
export type ProviderKind = (typeof PROVIDER_KINDS)[number];

/**
 * Static description of a provider kind, used by the server to talk to it and by
 * the UI to render the right form fields.
 */
export interface ProviderKindInfo {
  /** Provider identifier. */
  kind: ProviderKind;
  /** Human-readable product name; not localized because it is a brand. */
  label: string;
  /** Whether an API key must, may or cannot be supplied. */
  apiKey: 'required' | 'optional' | 'none';
  /**
   * `fixed`: only the public endpoint is used. `optional`: a custom endpoint may
   * replace the default. `required`: there is no sensible default endpoint.
   */
  baseUrl: 'fixed' | 'optional' | 'required';
  /** Endpoint used when the credential does not specify one. */
  defaultBaseUrl: string | null;
  /**
   * True when the endpoint is chosen by the user, i.e. the server would send
   * requests to an address it does not control. Gated by the admin setting
   * `allowCustomBaseUrls` to prevent server-side request forgery.
   */
  userDefinedEndpoint: boolean;
  /**
   * How every key of the provider starts, if they share a prefix; a key
   * without it was copied from somewhere else.
   */
  keyPrefix?: string;
}

/**
 * Whether an API key can belong to the provider, judged by its prefix.
 *
 * @param kind - The provider.
 * @param apiKey - The key as entered, trimmed.
 * @returns False if the provider's keys share a prefix the key lacks.
 */
export function plausibleKey(kind: ProviderKind, apiKey: string): boolean {
  const prefix = PROVIDERS[kind].keyPrefix;
  return prefix === undefined || apiKey.startsWith(prefix);
}

/**
 * Metadata for every supported provider, keyed by kind.
 */
export const PROVIDERS: Readonly<Record<ProviderKind, ProviderKindInfo>> = {
  anthropic: {
    kind: 'anthropic',
    label: 'Anthropic',
    apiKey: 'required',
    keyPrefix: 'sk-ant-',
    baseUrl: 'optional',
    defaultBaseUrl: 'https://api.anthropic.com/v1',
    userDefinedEndpoint: false,
  },
  openai: {
    kind: 'openai',
    label: 'OpenAI',
    apiKey: 'required',
    baseUrl: 'optional',
    defaultBaseUrl: 'https://api.openai.com/v1',
    userDefinedEndpoint: false,
  },
  google: {
    kind: 'google',
    label: 'Google Gemini',
    apiKey: 'required',
    baseUrl: 'optional',
    defaultBaseUrl: 'https://generativelanguage.googleapis.com/v1beta',
    userDefinedEndpoint: false,
  },
  mistral: {
    kind: 'mistral',
    label: 'Mistral',
    apiKey: 'required',
    baseUrl: 'optional',
    defaultBaseUrl: 'https://api.mistral.ai/v1',
    userDefinedEndpoint: false,
  },
  openrouter: {
    kind: 'openrouter',
    label: 'OpenRouter',
    apiKey: 'required',
    keyPrefix: 'sk-or-',
    baseUrl: 'fixed',
    defaultBaseUrl: 'https://openrouter.ai/api/v1',
    userDefinedEndpoint: false,
  },
  ollama: {
    kind: 'ollama',
    label: 'Ollama',
    apiKey: 'none',
    baseUrl: 'optional',
    defaultBaseUrl: 'http://localhost:11434/v1',
    userDefinedEndpoint: true,
  },
  'openai-compatible': {
    kind: 'openai-compatible',
    label: 'OpenAI-compatible',
    apiKey: 'optional',
    baseUrl: 'required',
    defaultBaseUrl: null,
    userDefinedEndpoint: true,
  },
};

/**
 * Decides whether a credential sends requests to an endpoint chosen by the user.
 *
 * @param kind - Provider kind of the credential.
 * @param baseUrl - Custom endpoint stored with the credential, if any.
 * @returns True if the endpoint is user-defined and therefore needs the
 *   `allowCustomBaseUrls` permission (admins are always allowed).
 */
export function usesCustomEndpoint(kind: ProviderKind, baseUrl: string | null | undefined): boolean {
  const info = PROVIDERS[kind];
  if (info.userDefinedEndpoint) {
    return true;
  }
  return Boolean(baseUrl) && baseUrl !== info.defaultBaseUrl;
}
