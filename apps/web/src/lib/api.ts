import type {
  AgentDataParts,
  ApiErrorBody,
  ApiErrorCode,
  ApprovalDecision,
  CompactRequestBody,
  ConversationDetail,
  ConversationSummary,
  CreateConversationInput,
  CreateProviderCredentialInput,
  CredentialModels,
  DeviceSummary,
  LinkConversationInput,
  MessageMetadata,
  PairingInfo,
  ProviderCredential,
  PublicSettings,
  SearchTestResult,
  ServerSettings,
  UpdateDeviceInput,
  UpdateLinkInput,
  UpdateProviderCredentialInput,
  UpdateServerSettings,
  VmStopResult,
} from '@harness/shared';
import type { UIMessage } from 'ai';
import type { MessageKey, TranslateFn } from '@/i18n/index.ts';

/**
 * Chat message type shared with the server.
 */
export type ChatMessage = UIMessage<MessageMetadata, AgentDataParts>;

/**
 * Error raised for non-2xx API responses and network failures.
 */
export class ApiError extends Error {
  /** HTTP status, or 0 if the server could not be reached. */
  readonly status: number;
  /** Stable error code, or `network` when no response arrived. */
  readonly code: ApiErrorCode | 'network';

  /**
   * @param status - HTTP status, 0 for network errors.
   * @param code - Error code from the response body.
   * @param message - English detail from the server.
   */
  constructor(status: number, code: ApiErrorCode | 'network', message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`/api${path}`, {
      method,
      credentials: 'same-origin',
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
  } catch (error) {
    throw new ApiError(0, 'network', error instanceof Error ? error.message : 'Network error');
  }
  if (!response.ok) {
    const payload = (await response.json().catch(() => null)) as ApiErrorBody | null;
    throw new ApiError(response.status, payload?.error.code ?? 'internal', payload?.error.message ?? response.statusText);
  }
  if (response.status === 204) {
    // Read to the end, or the browser reports the request as aborted.
    await response.text().catch(() => '');
    return undefined as T;
  }
  return (await response.json()) as T;
}

/**
 * Typed client for the app's JSON API (everything except `/api/auth`).
 */
export const api = {
  settings: {
    public: () => request<PublicSettings>('GET', '/settings'),
    get: () => request<ServerSettings>('GET', '/admin/settings'),
    update: (patch: UpdateServerSettings) => request<ServerSettings>('PATCH', '/admin/settings', patch),
    testSearch: () => request<SearchTestResult>('POST', '/admin/settings/search/test'),
  },
  providers: {
    list: () => request<ProviderCredential[]>('GET', '/providers'),
    create: (input: CreateProviderCredentialInput) => request<ProviderCredential>('POST', '/providers', input),
    update: (id: string, patch: UpdateProviderCredentialInput) =>
      request<ProviderCredential>('PATCH', `/providers/${encodeURIComponent(id)}`, patch),
    remove: (id: string) => request<void>('DELETE', `/providers/${encodeURIComponent(id)}`),
    test: (id: string) => request<CredentialModels>('GET', `/providers/${encodeURIComponent(id)}/models`),
  },
  models: {
    list: () => request<CredentialModels[]>('GET', '/models'),
  },
  devices: {
    list: () => request<DeviceSummary[]>('GET', '/devices'),
    remove: (id: string) => request<void>('DELETE', `/devices/${encodeURIComponent(id)}`),
    update: (id: string, input: UpdateDeviceInput) =>
      request<DeviceSummary>('PATCH', `/devices/${encodeURIComponent(id)}`, input),
    stopVm: (id: string) => request<VmStopResult>('POST', `/devices/${encodeURIComponent(id)}/vm/stop`),
    pairing: (userCode: string) => request<PairingInfo>('GET', `/devices/pairing/${encodeURIComponent(userCode)}`),
    decide: (userCode: string, approve: boolean) =>
      request<PairingInfo>('POST', `/devices/pairing/${encodeURIComponent(userCode)}`, { approve }),
  },
  conversations: {
    list: () => request<ConversationSummary[]>('GET', '/conversations'),
    get: (id: string) => request<ConversationDetail<ChatMessage>>('GET', `/conversations/${encodeURIComponent(id)}`),
    create: (input: CreateConversationInput) => request<ConversationSummary>('POST', '/conversations', input),
    rename: (id: string, title: string) =>
      request<ConversationSummary>('PATCH', `/conversations/${encodeURIComponent(id)}`, { title }),
    remove: (id: string) => request<void>('DELETE', `/conversations/${encodeURIComponent(id)}`),
    link: (id: string, input: LinkConversationInput) =>
      request<ConversationSummary>('PUT', `/conversations/${encodeURIComponent(id)}/link`, input),
    unlink: (id: string) => request<void>('DELETE', `/conversations/${encodeURIComponent(id)}/link`),
    updateLink: (id: string, input: UpdateLinkInput) =>
      request<ConversationSummary>('PATCH', `/conversations/${encodeURIComponent(id)}/link`, input),
    takeover: (id: string) => request<ChatMessage>('POST', `/conversations/${encodeURIComponent(id)}/takeover`),
    approve: (id: string, approvalId: number, decision: ApprovalDecision) =>
      request<void>('POST', `/conversations/${encodeURIComponent(id)}/approvals/${approvalId}`, { decision }),
    stop: (id: string) => request<void>('POST', `/conversations/${encodeURIComponent(id)}/stop`),
    compact: (id: string, input: CompactRequestBody) =>
      request<ChatMessage>('POST', `/conversations/${encodeURIComponent(id)}/compact`, input),
  },
};

/**
 * Turns any error into a localized, user-facing message.
 *
 * @param t - Translation function of the active language.
 * @param error - Thrown value.
 * @returns A sentence suitable for a toast or inline error.
 */
export function errorMessage(t: TranslateFn, error: unknown): string {
  if (error instanceof ApiError) {
    const key = `errors.${error.code}` as MessageKey;
    return t(key, { detail: error.message });
  }
  return t('errors.unknown');
}
