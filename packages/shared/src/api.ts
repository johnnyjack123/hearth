import { z } from 'zod';
import type { ChatLink } from './agent.ts';
import { PROVIDER_KINDS, type ProviderKind } from './providers.ts';

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
  | 'conflict'
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
}

/**
 * Server-wide settings managed by admins.
 */
export interface ServerSettings {
  /** Who may create an account. */
  registration: RegistrationMode;
  /** Whether non-admin users may point credentials at custom endpoints. */
  allowCustomBaseUrls: boolean;
}

/**
 * Validates a partial update of {@link ServerSettings}.
 */
export const updateServerSettingsSchema = z
  .object({
    registration: z.enum(['open', 'closed']),
    allowCustomBaseUrls: z.boolean(),
  })
  .partial()
  .strict();

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
  };
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
  })
  .partial()
  .strict();

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
});

/**
 * Body of a chat request.
 */
export type ChatRequestBody = z.infer<typeof chatRequestSchema>;
