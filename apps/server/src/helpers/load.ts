/**
 * Which helper models a chat request may use: the admin's choice of models
 * and an OpenRouter key of the user who chats.
 */
import { PROVIDERS } from '@hearth/shared';
import type { Kysely } from 'kysely';
import type { SecretBox } from '../crypto.ts';
import type { DB } from '../db/schema.ts';
import { type CredentialRow, listCredentialRows, resolveCredential } from '../services/credentials.ts';
import { getServerSettings } from '../services/settings.ts';
import { HELPER_TIMEOUT_MS, type Helpers } from './openrouter.ts';

/**
 * Overrides for tests, which point OpenRouter at a local stand-in. The
 * server itself never sets them.
 */
export interface HelperOptions {
  /** API root instead of the credential's. */
  apiUrl?: string;
  /** Time limit per call. */
  timeoutMs?: number;
}

/**
 * The helpers of a chat request. The chat's own credential is used if it
 * is an OpenRouter key, otherwise the user's oldest OpenRouter key: helper
 * calls are billed to the user, like the chat itself.
 *
 * @param db - Open database.
 * @param secretBox - Decrypts the key.
 * @param userId - The user who chats.
 * @param chat - Credential of the chat request; null for requests of a
 *   paired computer, which have no chat.
 * @param options - See {@link HelperOptions}.
 * @returns The helpers, or null if switched off or the user has no usable OpenRouter key.
 */
export async function loadHelpers(
  db: Kysely<DB>,
  secretBox: SecretBox,
  userId: string,
  chat: CredentialRow | null,
  options: HelperOptions = {},
): Promise<Helpers | null> {
  const { search } = await getServerSettings(db);
  if (!search.helpers.enabled) {
    return null;
  }
  const row =
    chat?.kind === 'openrouter' ? chat : (await listCredentialRows(db, userId)).find((candidate) => candidate.kind === 'openrouter');
  if (!row) {
    return null;
  }
  let key: string | null;
  try {
    key = resolveCredential(secretBox, row).apiKey;
  } catch {
    return null;
  }
  if (!key) {
    return null;
  }
  return {
    key,
    apiUrl: options.apiUrl ?? row.baseUrl ?? PROVIDERS.openrouter.defaultBaseUrl ?? 'https://openrouter.ai/api/v1',
    jevModel: search.helpers.jevModel,
    embeddingModel: search.helpers.embeddingModel,
    timeoutMs: options.timeoutMs ?? HELPER_TIMEOUT_MS,
    usage: [],
  };
}
