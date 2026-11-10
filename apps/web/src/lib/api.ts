import type {
  AgentDataParts,
  ApiErrorBody,
  ApiErrorCode,
  AppFileResult,
  ApprovalDecision,
  AppsSetResult,
  AppTemplate,
  ChatFeatures,
  ChatFile,
  CompactRequestBody,
  ConversationDetail,
  CostStatus,
  HostCheckStatus,
  OpenRouterHostAccess,
  OpenRouterHosts,
  ConversationSummary,
  CreateAppTemplateInput,
  CreateConversationInput,
  CreateProviderCredentialInput,
  CredentialModels,
  DeviceSummary,
  Inspection,
  LinkConversationInput,
  Memory,
  MemoryInput,
  MessageMetadata,
  PairingInfo,
  ProviderCredential,
  PublicSettings,
  RequestAppFileInput,
  SearchTestResult,
  ServerActivity,
  ServerSettings,
  UpdateAppTemplateInput,
  UpdateChatFeatures,
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

async function uploadFile(file: Blob, name: string): Promise<ChatFile> {
  let response: Response;
  try {
    // The raw bytes as the body; the server decides the type by the contents.
    response = await fetch('/api/files', {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'content-type': 'application/octet-stream', 'x-file-name': encodeURIComponent(name) },
      body: file,
    });
  } catch (error) {
    throw new ApiError(0, 'network', error instanceof Error ? error.message : 'Network error');
  }
  if (!response.ok) {
    const payload = (await response.json().catch(() => null)) as ApiErrorBody | null;
    throw new ApiError(response.status, payload?.error.code ?? 'internal', payload?.error.message ?? response.statusText);
  }
  return (await response.json()) as ChatFile;
}

/**
 * Typed client for the app's JSON API (everything except `/api/auth`).
 */
export const api = {
  files: {
    upload: uploadFile,
  },
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
    requestAppFile: (id: string, input: RequestAppFileInput) =>
      request<AppFileResult>('POST', `/devices/${encodeURIComponent(id)}/app-files`, input),
    setApps: (id: string, project: string, enabled: string[]) =>
      request<AppsSetResult>('PUT', `/devices/${encodeURIComponent(id)}/projects/${encodeURIComponent(project)}/apps`, { enabled }),
    pairing: (userCode: string) => request<PairingInfo>('GET', `/devices/pairing/${encodeURIComponent(userCode)}`),
    decide: (userCode: string, approve: boolean) =>
      request<PairingInfo>('POST', `/devices/pairing/${encodeURIComponent(userCode)}`, { approve }),
  },
  features: {
    get: () => request<ChatFeatures>('GET', '/features'),
    update: (patch: UpdateChatFeatures) => request<ChatFeatures>('PATCH', '/features', patch),
  },
  admin: {
    activity: () => request<ServerActivity>('GET', '/admin/activity'),
  },
  tags: {
    rename: (from: string, to: string) => request<{ chats: number }>('PATCH', '/tags', { from, to }),
    remove: (name: string) => request<{ chats: number }>('DELETE', '/tags', { name }),
  },
  openrouter: {
    hosts: (model: string) => request<OpenRouterHosts>('GET', `/openrouter/hosts?model=${encodeURIComponent(model)}`),
    pin: (model: string, host: string | null) => request<OpenRouterHosts>('PUT', '/openrouter/hosts', { model, host }),
    access: (model: string, credential: string) =>
      request<OpenRouterHostAccess>('GET', `/openrouter/access?model=${encodeURIComponent(model)}&credential=${encodeURIComponent(credential)}`),
    accessStatus: () => request<HostCheckStatus>('GET', '/openrouter/access/status'),
    refreshAccess: () => request<HostCheckStatus>('POST', '/openrouter/access/refresh'),
    setAccessCheck: (enabled: boolean) => request<HostCheckStatus>('PUT', '/openrouter/access/settings', { enabled }),
  },
  costs: {
    get: () => request<CostStatus>('GET', '/costs'),
    setLimit: (daily: number | null) => request<CostStatus>('PUT', '/costs/limit', { daily }),
  },
  memories: {
    list: () => request<Memory[]>('GET', '/memories'),
    create: (input: MemoryInput) => request<Memory>('POST', '/memories', input),
    update: (id: string, input: MemoryInput) => request<Memory>('PUT', `/memories/${encodeURIComponent(id)}`, input),
    remove: (id: string) => request<void>('DELETE', `/memories/${encodeURIComponent(id)}`),
    confirm: (id: string) => request<Memory>('POST', `/memories/${encodeURIComponent(id)}/confirm`),
  },
  appTemplates: {
    list: () => request<AppTemplate[]>('GET', '/app-templates'),
    create: (input: CreateAppTemplateInput) => request<AppTemplate>('POST', '/app-templates', input),
    update: (id: string, input: UpdateAppTemplateInput) => request<AppTemplate>('PUT', `/app-templates/${encodeURIComponent(id)}`, input),
    remove: (id: string) => request<void>('DELETE', `/app-templates/${encodeURIComponent(id)}`),
  },
  conversations: {
    list: () => request<ConversationSummary[]>('GET', '/conversations'),
    get: (id: string) => request<ConversationDetail<ChatMessage>>('GET', `/conversations/${encodeURIComponent(id)}`),
    create: (input: CreateConversationInput) => request<ConversationSummary>('POST', '/conversations', input),
    rename: (id: string, title: string) =>
      request<ConversationSummary>('PATCH', `/conversations/${encodeURIComponent(id)}`, { title }),
    remove: (id: string) => request<void>('DELETE', `/conversations/${encodeURIComponent(id)}`),
    setTags: (id: string, tags: string[]) => request<ConversationSummary>('PUT', `/conversations/${encodeURIComponent(id)}/tags`, { tags }),
    link: (id: string, input: LinkConversationInput) =>
      request<ConversationSummary>('PUT', `/conversations/${encodeURIComponent(id)}/link`, input),
    unlink: (id: string) => request<void>('DELETE', `/conversations/${encodeURIComponent(id)}/link`),
    updateLink: (id: string, input: UpdateLinkInput) =>
      request<ConversationSummary>('PATCH', `/conversations/${encodeURIComponent(id)}/link`, input),
    takeover: (id: string) => request<ChatMessage>('POST', `/conversations/${encodeURIComponent(id)}/takeover`),
    approve: (id: string, approvalId: number, decision: ApprovalDecision) =>
      request<void>('POST', `/conversations/${encodeURIComponent(id)}/approvals/${approvalId}`, { decision }),
    stop: (id: string) => request<void>('POST', `/conversations/${encodeURIComponent(id)}/stop`),
    assess: (id: string, messageId: string, toolCallId: string) =>
      request<ChatMessage>('POST', `/conversations/${encodeURIComponent(id)}/assessments`, { messageId, toolCallId }),
    inspection: (id: string, messageId: string, toolCallId: string) =>
      request<Inspection>(
        'GET',
        `/conversations/${encodeURIComponent(id)}/inspection?message=${encodeURIComponent(messageId)}&tool=${encodeURIComponent(toolCallId)}`,
      ),
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
