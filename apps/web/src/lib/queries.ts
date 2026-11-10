import { queryOptions, useQuery } from '@tanstack/react-query';
import { api } from './api.ts';
import { authClient } from './auth-client.ts';

/**
 * Query keys in one place so invalidation stays consistent.
 */
export const queryKeys = {
  session: ['session'] as const,
  publicSettings: ['settings', 'public'] as const,
  serverSettings: ['settings', 'server'] as const,
  providers: ['providers'] as const,
  models: ['models'] as const,
  devices: ['devices'] as const,
  appTemplates: ['app-templates'] as const,
  memories: ['memories'] as const,
  costs: ['costs'] as const,
  hostCheck: ['openrouter-access', 'status'] as const,
  features: ['features'] as const,
  activity: ['admin', 'activity'] as const,
  conversations: ['conversations'] as const,
  conversation: (id: string) => ['conversations', id] as const,
  users: ['admin', 'users'] as const,
};

/**
 * Current session, or null when signed out. Shared by route guards and UI.
 */
export const sessionQuery = queryOptions({
  queryKey: queryKeys.session,
  queryFn: async () => {
    const { data } = await authClient.getSession();
    return data ?? null;
  },
  staleTime: 60_000,
});

/**
 * Public server settings (registration mode, whether accounts exist).
 */
export const publicSettingsQuery = queryOptions({
  queryKey: queryKeys.publicSettings,
  queryFn: api.settings.public,
  staleTime: 30_000,
});

/**
 * Reads the current session.
 *
 * @returns The query result; `data` is null when signed out.
 */
export function useSession() {
  return useQuery(sessionQuery);
}

/**
 * Lists the user's conversations, newest activity first.
 *
 * @returns The query result.
 */
export function useConversations() {
  return useQuery({ queryKey: queryKeys.conversations, queryFn: api.conversations.list });
}

/**
 * Loads one conversation with its messages.
 *
 * @param id - Conversation id.
 * @returns The query result.
 */
export function useConversation(id: string) {
  return useQuery({
    queryKey: queryKeys.conversation(id),
    queryFn: () => api.conversations.get(id),
    // Messages live in useChat once loaded; refetching would reset a running stream.
    staleTime: Number.POSITIVE_INFINITY,
    retry: (count, error) => (error as { status?: number }).status !== 404 && count < 2,
  });
}

/**
 * Lists the user's provider credentials.
 *
 * @returns The query result.
 */
export function useProviders() {
  return useQuery({ queryKey: queryKeys.providers, queryFn: api.providers.list });
}

/** Online state and VM change on the computer; this keeps them current. */
const DEVICE_REFRESH_MS = 5_000;

/**
 * Lists the user's paired devices.
 *
 * @param live - Refresh every few seconds, for views that show online state
 *   and VM.
 * @returns The query result.
 */
export function useDevices(live = false) {
  return useQuery({
    queryKey: queryKeys.devices,
    queryFn: api.devices.list,
    refetchInterval: live ? DEVICE_REFRESH_MS : false,
  });
}

/**
 * The user's memories.
 *
 * @returns The query result.
 */
export function useMemories() {
  return useQuery({ queryKey: queryKeys.memories, queryFn: api.memories.list });
}

/**
 * What the user spent today and the daily limit.
 *
 * @returns The query result.
 */
export function useCosts() {
  return useQuery({ queryKey: queryKeys.costs, queryFn: api.costs.get });
}

/**
 * The account's own programs.
 *
 * @returns The query result.
 */
export function useAppTemplates() {
  return useQuery({ queryKey: queryKeys.appTemplates, queryFn: api.appTemplates.list });
}

/**
 * Lists models per credential. Cached for a few minutes because providers
 * rarely change their catalog and the server also caches.
 *
 * @param enabled - Skip fetching until needed.
 * @returns The query result.
 */
export function useModels(enabled = true) {
  return useQuery({ queryKey: queryKeys.models, queryFn: api.models.list, enabled, staleTime: 5 * 60_000 });
}

/**
 * Reads the public server settings.
 *
 * @returns The query result.
 */
export function usePublicSettings() {
  return useQuery(publicSettingsQuery);
}
