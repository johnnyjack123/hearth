import { z } from 'zod';
import { type ChatLink, linkConversationSchema } from './agent.ts';
import { PROVIDER_KINDS, type ProviderKind } from './providers.ts';
import { type SearchSettings, updateSearchSettingsSchema } from './web-search.ts';

/**
 * Machine-readable error codes returned by the API. The UI maps them to
 * localized messages; the accompanying `message` is English and meant for logs.
 */
export type ApiErrorCode =
  | 'unauthorized'
  | 'forbidden'
  | 'not_found'
  | 'validation'
  | 'provider_error'
  | 'custom_base_url_forbidden'
  | 'too_many_requests'
  | 'device_offline'
  | 'device_outdated'
  | 'device_timeout'
  | 'device_failed'
  | 'conflict'
  | 'app_name_taken'
  | 'too_many_apps'
  | 'file_too_large'
  | 'file_unsupported'
  | 'file_empty'
  | 'converter_unavailable'
  | 'storage_unavailable'
  | 'memory_full'
  | 'memory_rejected'
  | 'memory_off'
  | 'convert_failed'
  | 'no_text'
  | 'internal';

/**
 * Error envelope of every non-2xx JSON response.
 */
export interface ApiErrorBody {
  /** Error details. */
  error: {
    /** Stable code for programmatic handling. */
    code: ApiErrorCode;
    /** English description for logs and developer tooling. */
    message: string;
  };
}

/**
 * Who may create an account. `closed` still lets admins create users.
 */
export type RegistrationMode = 'open' | 'closed';

/**
 * Settings that anyone, including signed-out visitors, may read.
 */
export interface PublicSettings {
  /** Current registration mode. */
  registration: RegistrationMode;
  /** False until the first account exists; that account becomes admin. */
  hasUsers: boolean;
  /** Whether non-admin users may use custom endpoints (Ollama, OpenAI-compatible). */
  allowCustomBaseUrls: boolean;
  /** Whether a search provider is set up, so chats can offer research mode. */
  webSearch: boolean;
}

/**
 * Server-wide settings managed by admins.
 */
export interface ServerSettings {
  /** Who may create an account. */
  registration: RegistrationMode;
  /** Whether non-admin users may point credentials at custom endpoints. */
  allowCustomBaseUrls: boolean;
  /** Whether models in ordinary chats may load web pages through the server. */
  webFetch: boolean;
  /** The calculator tool (math.js on the server) in ordinary chats. */
  calculator: boolean;
  /**
   * OpenRouter requests go to the host that is cheapest right now (its
   * discounts, cache prices and hours included), the next ones as fallback;
   * off leaves the choice to OpenRouter.
   */
  openRouterCheapest: boolean;
  /** With {@link openRouterCheapest}: skip hosts that run a model in 4 bits (fp4, int4), which may answer worse. */
  openRouterNoFourBit: boolean;
  /** The memory: chat models and agents may store what the user wants every chat to know. */
  memory: boolean;
  /** The progress list in ordinary chats: the model shows its plan for tasks with several steps. */
  progress: boolean;
  /** Web search for research mode. */
  search: SearchSettings;
  /** When long chats get older pages and searches only in short form. */
  condense: CondenseSettings;
  /** When long chats get their older turns summarized. */
  compact: CompactSettings;
}

/**
 * How hard a model thinks before it answers, named as in pi. A chat without
 * a level ("Standard") sends nothing and gets the provider's default.
 */
export const EFFORT_LEVELS = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'] as const;

/** See {@link EFFORT_LEVELS}. */
export type EffortLevel = (typeof EFFORT_LEVELS)[number];

/**
 * When a chat gets its older turns summarized, like pi does it: once fewer
 * than {@link reserveTokens} of the model's context window are left, the
 * turns before the most recent {@link keepRecentTokens} become a summary
 * the model reads instead. The user can also ask for it at any time.
 */
export interface CompactSettings {
  /** Whether chats are summarized automatically; asking for it works anyway. */
  enabled: boolean;
  /** Tokens of the context window that stay free; less left starts a summary. */
  reserveTokens: number;
  /** Tokens of the latest turns that the model keeps reading word for word. */
  keepRecentTokens: number;
}

/** Validates a change of {@link CompactSettings}. */
export const updateCompactSettingsSchema = z
  .object({
    enabled: z.boolean(),
    reserveTokens: z.number().int().min(1_000).max(1_000_000),
    keepRecentTokens: z.number().int().min(0).max(1_000_000),
  })
  .partial()
  .strict();

/**
 * When a new message condenses a chat: earlier pages then reach the model as
 * URL and title only. Condensing breaks the provider's prompt cache once,
 * from the first shortened page on; with cache hits at 1/50 of the price,
 * carrying a long chat is cheap, so the thresholds sit high.
 */
export interface CondenseSettings {
  /** Whether chats are condensed at all. */
  enabled: boolean;
  /** Size of the conversation after the last reply, in tokens. */
  atTokens: number;
  /** The same as a share (0.1 to 1) of the model's context window, for models with a small one; the lower limit applies. */
  atWindowShare: number;
  /** Hours after the last reply, when the cache has usually gone cold and condensing costs nothing; 0 for never. */
  afterIdleHours: number;
}

/** Validates a change of {@link CondenseSettings}. */
export const updateCondenseSettingsSchema = z
  .object({
    enabled: z.boolean(),
    atTokens: z.number().int().min(10_000).max(10_000_000),
    atWindowShare: z.number().min(0.1).max(1),
    afterIdleHours: z.number().min(0).max(720),
  })
  .partial()
  .strict();

/**
 * Validates a partial update of {@link ServerSettings}.
 */
export const updateServerSettingsSchema = z
  .object({
    registration: z.enum(['open', 'closed']),
    allowCustomBaseUrls: z.boolean(),
    webFetch: z.boolean(),
    calculator: z.boolean(),
    openRouterCheapest: z.boolean(),
    openRouterNoFourBit: z.boolean(),
    memory: z.boolean(),
    progress: z.boolean(),
    search: updateSearchSettingsSchema,
    condense: updateCondenseSettingsSchema,
    compact: updateCompactSettingsSchema,
  })
  .partial()
  .strict();

/** See {@link updateServerSettingsSchema}. */
export type UpdateServerSettings = z.infer<typeof updateServerSettingsSchema>;

/**
 * A stored provider credential as returned to its owner. The API key itself is
 * never returned, only a short hint to tell keys apart.
 */
export interface ProviderCredential {
  /** Credential id. */
  id: string;
  /** Provider kind. */
  kind: ProviderKind;
  /** User-chosen display name. */
  label: string;
  /** Custom endpoint, or null for the provider default. */
  baseUrl: string | null;
  /** Last characters of the key (e.g. `…f3a9`), or null if no key is stored. */
  keyHint: string | null;
  /** Creation time, epoch milliseconds. */
  createdAt: number;
}

const baseUrlSchema = z
  .url({ protocol: /^https?$/ })
  .max(500)
  .transform((value) => value.replace(/\/+$/, ''));

/**
 * Validates the body of a request that stores a new provider credential.
 */
export const createProviderCredentialSchema = z
  .object({
    kind: z.enum(PROVIDER_KINDS),
    label: z.string().trim().min(1).max(60),
    apiKey: z.string().trim().min(1).max(500).optional(),
    baseUrl: baseUrlSchema.optional(),
  })
  .strict();

/**
 * Input accepted when creating a provider credential.
 */
export type CreateProviderCredentialInput = z.infer<typeof createProviderCredentialSchema>;

/**
 * Validates the body of a request that changes a stored provider credential.
 * `baseUrl: null` resets the endpoint to the provider default.
 */
export const updateProviderCredentialSchema = z
  .object({
    label: z.string().trim().min(1).max(60),
    apiKey: z.string().trim().min(1).max(500),
    baseUrl: baseUrlSchema.nullable(),
  })
  .partial()
  .strict();

/**
 * Input accepted when updating a provider credential.
 */
export type UpdateProviderCredentialInput = z.infer<typeof updateProviderCredentialSchema>;

/**
 * A model offered by one of the user's credentials.
 */
export interface ModelInfo {
  /** Provider-specific model id, sent back unchanged when chatting. */
  id: string;
  /** Display name; falls back to the id when the provider has none. */
  name: string;
  /**
   * Whether the model reads images, as the models.dev catalog says;
   * missing when unknown (local models, other endpoints).
   */
  images?: boolean;
}

/**
 * Models available through one credential. Listing failures are reported per
 * credential so one broken key does not hide the others.
 */
export interface CredentialModels {
  /** Credential id. */
  credentialId: string;
  /** Provider kind of the credential. */
  kind: ProviderKind;
  /** Credential display name. */
  label: string;
  /** Available models, sorted by name. Empty when `error` is set. */
  models: ModelInfo[];
  /** English error description if the provider could not be queried. */
  error: string | null;
}

/**
 * Identifies the model a conversation should use.
 */
export interface ModelSelection {
  /** Credential that pays for and authenticates the request. */
  credentialId: string;
  /** Provider-specific model id. */
  modelId: string;
}

/**
 * Metadata attached to each stored chat message.
 */
export interface MessageMetadata {
  /** Creation time, epoch milliseconds. */
  createdAt?: number;
  /** Model that produced an assistant message. */
  modelId?: string;
  /** Credential used for an assistant message. */
  credentialId?: string;
  /**
   * Set on user messages sent in research mode, so the chat opens with the
   * switch as it was last used.
   */
  research?: boolean;
  /**
   * Thinking effort a user message was sent with; missing for the
   * provider's default. The chat opens with it.
   */
  effort?: EffortLevel;
  /** Date (YYYY-MM-DD) a user message of an ordinary chat was sent on, for the notes. */
  date?: string;
  /**
   * Fingerprint of the user's memories when a user message was sent; the
   * model gets them again only when it changes (empty string: none).
   */
  memoryHash?: string;
  /**
   * Note of LLM Harness the model reads in front of a user message, such as
   * the date or a change of research mode; stored, so it never changes.
   */
  note?: string;
  /**
   * Set when the chat had grown long. On a user message: results of
   * web_fetch and web_search before it reach the model only in short form,
   * from now on. On the reply to it: so the chat can say so.
   */
  condensed?: boolean;
  /**
   * How long each tool call of the reply ran, in milliseconds by tool call
   * id, from its start to its result; a web fetch includes the wait for the
   * user's answer. Missing for replies from before 31.10.2026.
   */
  toolMs?: Record<string, number>;
  /** Set on replies of an agent in a linked environment. */
  agent?: {
    /** Project the agent worked in. */
    project: string;
    /** Harness that produced the reply. */
    harness: string;
  };
  /** Token usage reported by the provider, if any. */
  usage?: {
    /** Prompt tokens, summed over all model calls of the reply. */
    inputTokens?: number;
    /** Completion tokens, summed over all model calls of the reply. */
    outputTokens?: number;
    /** Part of the prompt tokens read from the provider's cache. */
    cachedInputTokens?: number;
    /** Size of the conversation after the reply: what the next message starts from. */
    contextTokens?: number;
    /** Context window of the model, if known. */
    contextWindow?: number;
    /** Cost of the reply in US dollars, if the model's prices are known. */
    cost?: number;
    /** Part of the completion tokens spent on reasoning, if the provider says. */
    reasoningTokens?: number;
    /**
     * Each model call of the reply in order, for the usage details; missing
     * in replies stored before 2026-10-20.
     */
    calls?: ModelCallUsage[];
    /** Prices the cost was computed with, US dollars per million tokens. */
    rates?: TokenRates;
    /**
     * True when {@link cost} is what the provider billed (OpenRouter reports
     * it with every answer), not an estimate from catalog prices.
     */
    billed?: boolean;
    /**
     * Calls of helper models for this reply (Jev, embeddings, the title);
     * their cost is part of {@link cost}.
     */
    helpers?: ModelCallUsage[];
  };
}

/** Prices of a model, US dollars per million tokens. */
export interface TokenRates {
  /** Input not read from the cache. */
  input: number;
  /** Output, reasoning included. */
  output: number;
  /** Input read from the cache, if the provider prices it apart. */
  cacheRead?: number;
  /** Input written to the cache, if the provider prices it apart. */
  cacheWrite?: number;
}

/**
 * Tokens and cost of one model call within a reply. A reply with tools
 * makes several calls, each sending the whole conversation so far again.
 */
export interface ModelCallUsage {
  /** Prompt tokens, cached ones included. */
  input: number;
  /** Part of {@link input} read from the provider's cache. */
  cacheRead: number;
  /** Part of {@link input} written to the provider's cache. */
  cacheWrite: number;
  /** Completion tokens, reasoning included. */
  output: number;
  /** Part of {@link output} spent on reasoning, if the provider says. */
  reasoning?: number;
  /** Tools the call asked for; empty when it ended with the answer. */
  tools: string[];
  /** Model of a helper call; the reply's own calls use the chat's model. */
  model?: string;
  /** What a helper call was for. */
  purpose?: 'rank' | 'sections' | 'title' | 'review' | 'compact' | 'assess';
  /**
   * Cost in US dollars, split by kind where the prices are known; agents
   * report only the total.
   */
  cost?: { total: number; input?: number; cacheRead?: number; cacheWrite?: number; output?: number };
  /** True when {@link cost} is what the provider billed, not an estimate. */
  billed?: boolean;
  /** Who ran the call, when OpenRouter says: the host it routed to, e.g. `DeepInfra`. */
  host?: string;
  /**
   * The prices of the call, US dollars per million tokens: the host's list
   * price at OpenRouter at the time of a billed call, else the catalog's at
   * the time of the call (DeepSeek's night prices included).
   */
  rates?: TokenRates;
  /**
   * Milliseconds the model wrote, from its first token to the end of the
   * call; with {@link output} the speed in tokens per second. Missing for
   * helper calls and replies from before 30.10.2026.
   */
  ms?: number;
}

/**
 * A conversation as shown in the sidebar.
 */
export interface ConversationSummary {
  /** Conversation id. */
  id: string;
  /** Generated or user-set title; null until the first reply arrives. */
  title: string | null;
  /** Last selected credential, if any. */
  credentialId: string | null;
  /** Last selected model, if any. */
  modelId: string | null;
  /** Environment the chat is linked to; null for an ordinary chat. */
  link: ChatLink | null;
  /** Creation time, epoch milliseconds. */
  createdAt: number;
  /** Time of the last message or rename, epoch milliseconds. */
  updatedAt: number;
}

/**
 * A conversation including its messages.
 *
 * @typeParam TMessage - Message type; the web app and server both use the AI
 *   SDK `UIMessage<MessageMetadata>`, which this package does not depend on.
 */
export interface ConversationDetail<TMessage = unknown> extends ConversationSummary {
  /** Messages in chronological order. */
  messages: TMessage[];
}

/**
 * Validates the body of a request that creates a conversation.
 */
export const createConversationSchema = z
  .object({
    credentialId: z.string().min(1).max(100),
    modelId: z.string().min(1).max(200),
    /** Environment to link the new chat to, so it starts with an agent. */
    link: linkConversationSchema,
  })
  .partial()
  .strict();

/** See {@link createConversationSchema}. */
export type CreateConversationInput = z.infer<typeof createConversationSchema>;

/**
 * Validates the body of a request that renames a conversation.
 */
export const updateConversationSchema = z
  .object({
    title: z.string().trim().min(1).max(200),
  })
  .strict();

/**
 * Validates the body of a chat request. The message itself is validated with
 * the AI SDK on the server, so it is only checked for presence here.
 */
export const chatRequestSchema = z.object({
  message: z.record(z.string(), z.unknown()),
  credentialId: z.string().min(1).max(100),
  modelId: z.string().min(1).max(200),
  /**
   * Research mode: the model may search, and pages from search results,
   * the user's messages and links on loaded pages load without asking.
   */
  research: z.boolean().optional(),
  /** Thinking effort; missing for the provider's default. */
  effort: z.enum(EFFORT_LEVELS).optional(),
  /** Jev judges prompt injection warnings right away (needs an OpenRouter key). */
  assess: z.boolean().optional(),
});

/**
 * Validates a request for Jev's opinion on the prompt injection warning of
 * one tool call in a stored reply.
 */
export const assessmentRequestSchema = z
  .object({
    messageId: z.string().min(1).max(100),
    toolCallId: z.string().min(1).max(200),
  })
  .strict();

/** See {@link assessmentRequestSchema}. */
export type AssessmentRequest = z.infer<typeof assessmentRequestSchema>;

/**
 * Validates the body of a request that summarizes a chat now: the model
 * that writes the summary, as in a chat request.
 */
export const compactRequestSchema = z
  .object({
    credentialId: z.string().min(1).max(100),
    modelId: z.string().min(1).max(200),
  })
  .strict();

/** See {@link compactRequestSchema}. */
export type CompactRequestBody = z.infer<typeof compactRequestSchema>;

/**
 * Body of a chat request.
 */
export type ChatRequestBody = z.infer<typeof chatRequestSchema>;
