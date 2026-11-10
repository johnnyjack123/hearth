import {
  type CreateProviderCredentialInput,
  PROVIDERS,
  type ProviderCredential,
  type UpdateProviderCredentialInput,
} from '@hearth/shared';
import type { Kysely, Selectable } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import { keyHint, type SecretBox } from '../crypto.ts';
import type { DB, ProviderCredentialTable } from '../db/schema.ts';
import { HttpError } from '../http/errors.ts';
import type { ResolvedCredential } from '../llm.ts';

/**
 * A credential row as read from the database.
 */
export type CredentialRow = Selectable<ProviderCredentialTable>;

function toDto(row: CredentialRow): ProviderCredential {
  return {
    id: row.id,
    kind: row.kind,
    label: row.label,
    baseUrl: row.baseUrl,
    keyHint: row.keyHint,
    createdAt: row.createdAt,
  };
}

function encryptionContext(userId: string, credentialId: string): string {
  return `${userId}:${credentialId}`;
}

function assertShape(kind: CredentialRow['kind'], apiKey: string | null, baseUrl: string | null): void {
  const info = PROVIDERS[kind];
  if (info.apiKey === 'required' && !apiKey) {
    throw new HttpError(400, 'validation', `${info.label} requires an API key`);
  }
  if (info.baseUrl === 'required' && !baseUrl) {
    throw new HttpError(400, 'validation', `${info.label} requires a base URL`);
  }
  if (info.baseUrl === 'fixed' && baseUrl) {
    throw new HttpError(400, 'validation', `${info.label} does not support a custom base URL`);
  }
}

/**
 * Lists a user's credentials, oldest first.
 *
 * @param db - Open database.
 * @param userId - Owner.
 * @returns Credentials without their keys.
 */
export async function listCredentials(db: Kysely<DB>, userId: string): Promise<ProviderCredential[]> {
  return (await listCredentialRows(db, userId)).map(toDto);
}

/**
 * Lists a user's credential rows including encrypted keys, oldest first.
 * Server-internal; never return these rows to a client.
 *
 * @param db - Open database.
 * @param userId - Owner.
 * @returns Credential rows.
 */
export async function listCredentialRows(db: Kysely<DB>, userId: string): Promise<CredentialRow[]> {
  return db
    .selectFrom('providerCredential')
    .selectAll()
    .where('userId', '=', userId)
    .orderBy('createdAt', 'asc')
    .execute();
}

/**
 * Loads one credential row if it belongs to the user.
 *
 * @param db - Open database.
 * @param userId - Expected owner.
 * @param id - Credential id.
 * @returns The row, or undefined if it does not exist or belongs to someone else.
 */
export async function findCredential(db: Kysely<DB>, userId: string, id: string): Promise<CredentialRow | undefined> {
  return db
    .selectFrom('providerCredential')
    .selectAll()
    .where('id', '=', id)
    .where('userId', '=', userId)
    .executeTakeFirst();
}

/**
 * Stores a new credential with its key encrypted.
 *
 * @param db - Open database.
 * @param box - Encryption for the API key.
 * @param userId - Owner.
 * @param input - Validated input.
 * @returns The created credential without its key.
 * @throws {HttpError} If the input does not fit the provider kind.
 */
export async function createCredential(
  db: Kysely<DB>,
  box: SecretBox,
  userId: string,
  input: CreateProviderCredentialInput,
): Promise<ProviderCredential> {
  const apiKey = PROVIDERS[input.kind].apiKey === 'none' ? null : (input.apiKey ?? null);
  const baseUrl = input.baseUrl ?? null;
  assertShape(input.kind, apiKey, baseUrl);

  const id = uuidv7();
  const now = Date.now();
  const row: CredentialRow = {
    id,
    userId,
    kind: input.kind,
    label: input.label,
    baseUrl,
    encryptedKey: apiKey ? box.encrypt(apiKey, encryptionContext(userId, id)) : null,
    keyHint: apiKey ? keyHint(apiKey) : null,
    createdAt: now,
    updatedAt: now,
  };
  await db.insertInto('providerCredential').values(row).execute();
  return toDto(row);
}

/**
 * Applies a partial update to a credential.
 *
 * @param db - Open database.
 * @param box - Encryption for a replaced API key.
 * @param row - Current row, already checked for ownership.
 * @param patch - Validated changes.
 * @returns The updated credential without its key.
 * @throws {HttpError} If the result would not fit the provider kind.
 */
export async function updateCredential(
  db: Kysely<DB>,
  box: SecretBox,
  row: CredentialRow,
  patch: UpdateProviderCredentialInput,
): Promise<ProviderCredential> {
  const next: CredentialRow = { ...row, updatedAt: Date.now() };
  if (patch.label !== undefined) {
    next.label = patch.label;
  }
  if (patch.baseUrl !== undefined) {
    next.baseUrl = patch.baseUrl;
  }
  if (patch.apiKey !== undefined && PROVIDERS[row.kind].apiKey !== 'none') {
    next.encryptedKey = box.encrypt(patch.apiKey, encryptionContext(row.userId, row.id));
    next.keyHint = keyHint(patch.apiKey);
  }
  assertShape(next.kind, next.encryptedKey, next.baseUrl);

  await db
    .updateTable('providerCredential')
    .set({
      label: next.label,
      baseUrl: next.baseUrl,
      encryptedKey: next.encryptedKey,
      keyHint: next.keyHint,
      updatedAt: next.updatedAt,
    })
    .where('id', '=', row.id)
    .execute();
  return toDto(next);
}

/**
 * Deletes a credential. Conversations that used it keep their history but
 * lose the model selection.
 *
 * @param db - Open database.
 * @param row - Row to delete, already checked for ownership.
 */
export async function deleteCredential(db: Kysely<DB>, row: CredentialRow): Promise<void> {
  await db.deleteFrom('providerCredential').where('id', '=', row.id).execute();
}

/**
 * Decrypts a credential for use with a provider.
 *
 * @param box - Encryption that produced the stored key.
 * @param row - Credential row.
 * @returns The credential including its plaintext key.
 * @throws {HttpError} If the stored key cannot be decrypted, e.g. after the
 *   server secret changed.
 */
export function resolveCredential(box: SecretBox, row: CredentialRow): ResolvedCredential {
  let apiKey: string | null = null;
  if (row.encryptedKey) {
    try {
      apiKey = box.decrypt(row.encryptedKey, encryptionContext(row.userId, row.id));
    } catch {
      throw new HttpError(409, 'provider_error', 'Stored API key cannot be decrypted; please enter it again');
    }
  }
  return { id: row.id, kind: row.kind, baseUrl: row.baseUrl, apiKey };
}
