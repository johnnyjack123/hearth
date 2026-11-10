import { type HostCheckStatus, type OpenRouterHostAccess, PROVIDERS } from '@hearth/shared';
import type { Kysely } from 'kysely';
import type { SecretBox } from '../crypto.ts';
import type { DB } from '../db/schema.ts';
import type { OpenRouterRouting } from '../openrouter.ts';
import type { HostAccessState, HostProber } from '../openrouter-access.ts';
import { findCredential, resolveCredential } from './credentials.ts';

/** A model's hosts are checked again after this long; the user's wish (9.11.): once a week. */
export const HOST_CHECK_MAX_AGE_MS = 7 * 24 * 60 * 60_000;

/** Only the models of the latest chats are checked, so probes stay few. */
const RECENT_MODELS = 5;

/** The per-user setting that switches the checks off. */
const HOST_CHECK_SETTING = 'openRouterHostCheck';

/**
 * Whether the user lets the server check which hosts their OpenRouter
 * account allows; on unless switched off.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @returns True if on.
 */
export async function hostCheckEnabled(db: Kysely<DB>, userId: string): Promise<boolean> {
  const row = await db.selectFrom('userSetting').select('value').where('userId', '=', userId).where('key', '=', HOST_CHECK_SETTING).executeTakeFirst();
  return row ? JSON.parse(row.value) !== false : true;
}

/**
 * Switches the checks on or off. Off also forgets what they found, so
 * neither the list nor the routing leaves any host out.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @param enabled - The new state.
 */
export async function setHostCheck(db: Kysely<DB>, userId: string, enabled: boolean): Promise<void> {
  await db.transaction().execute(async (trx) => {
    await trx.deleteFrom('userSetting').where('userId', '=', userId).where('key', '=', HOST_CHECK_SETTING).execute();
    await trx.insertInto('userSetting').values({ userId, key: HOST_CHECK_SETTING, value: JSON.stringify(enabled), updatedAt: Date.now() }).execute();
    if (!enabled) {
      const credentials = trx.selectFrom('providerCredential').select('id').where('userId', '=', userId);
      await trx.deleteFrom('openRouterAccess').where('credentialId', 'in', credentials).execute();
    }
  });
}

/**
 * What the last check found for a credential and model.
 *
 * @param db - Open database.
 * @param credentialId - The OpenRouter credential.
 * @param modelId - OpenRouter's model id.
 * @returns The result, or null if never checked.
 */
export async function storedAccess(db: Kysely<DB>, credentialId: string, modelId: string): Promise<OpenRouterHostAccess | null> {
  const row = await db
    .selectFrom('openRouterAccess')
    .selectAll()
    .where('credentialId', '=', credentialId)
    .where('modelId', '=', modelId)
    .executeTakeFirst();
  return row ? { blocked: JSON.parse(row.blocked) as string[], allowed: JSON.parse(row.allowed) as string[], checked: true, checkedAt: row.checkedAt } : null;
}

/**
 * Hosts a credential may not use for a model, for the cheapest routing;
 * none while the checks are off or never ran.
 *
 * @param db - Open database.
 * @param userId - Owner of the credential.
 * @param credentialId - The OpenRouter credential.
 * @param modelId - OpenRouter's model id.
 * @returns Their slugs.
 */
export async function blockedHosts(db: Kysely<DB>, userId: string, credentialId: string, modelId: string): Promise<Set<string>> {
  if (!(await hostCheckEnabled(db, userId))) {
    return new Set();
  }
  return new Set((await storedAccess(db, credentialId, modelId))?.blocked ?? []);
}

async function saveAccess(db: Kysely<DB>, credentialId: string, modelId: string, states: Map<string, HostAccessState>): Promise<OpenRouterHostAccess> {
  const of = (state: HostAccessState) => [...states].filter(([, value]) => value === state).map(([slug]) => slug);
  const [blocked, allowed, checkedAt] = [of('blocked'), of('allowed'), Date.now()];
  await db.transaction().execute(async (trx) => {
    await trx.deleteFrom('openRouterAccess').where('credentialId', '=', credentialId).where('modelId', '=', modelId).execute();
    await trx
      .insertInto('openRouterAccess')
      .values({ credentialId, modelId, blocked: JSON.stringify(blocked), allowed: JSON.stringify(allowed), checkedAt })
      .execute();
  });
  return { blocked, allowed, checked: true, checkedAt };
}

/**
 * The OpenRouter models of the user's latest chats, each with the
 * credential it ran on, newest first.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @returns Up to five credential and model pairs.
 */
export async function recentOpenRouterModels(db: Kysely<DB>, userId: string): Promise<{ credentialId: string; credential: string; modelId: string }[]> {
  const rows = await db
    .selectFrom('conversation')
    .innerJoin('providerCredential', 'providerCredential.id', 'conversation.credentialId')
    .select(['conversation.credentialId', 'conversation.modelId', 'providerCredential.label'])
    .where('conversation.userId', '=', userId)
    .where('providerCredential.kind', '=', 'openrouter')
    .where('conversation.modelId', 'is not', null)
    .orderBy('conversation.updatedAt', 'desc')
    .limit(200)
    .execute();
  const recent: { credentialId: string; credential: string; modelId: string }[] = [];
  for (const row of rows) {
    if (row.credentialId && row.modelId && !recent.some((entry) => entry.credentialId === row.credentialId && entry.modelId === row.modelId)) {
      recent.push({ credentialId: row.credentialId, credential: row.label, modelId: row.modelId });
    }
    if (recent.length === RECENT_MODELS) {
      break;
    }
  }
  return recent;
}

/** What checking needs besides the database. */
export interface HostCheckDeps {
  /** Decrypts the credential's key. */
  secretBox: SecretBox;
  /** Lists the hosts of a model. */
  routing: OpenRouterRouting | undefined;
  /** Asks the hosts. */
  prober: HostProber | undefined;
}

/**
 * Checks the hosts of a model for a credential now and stores the result.
 *
 * @param db - Open database.
 * @param deps - Key decryption, host list and prober.
 * @param userId - Owner of the credential.
 * @param credentialId - The OpenRouter credential.
 * @param modelId - OpenRouter's model id.
 * @returns The result, or null when nothing could be checked (no host list, no key, not OpenRouter).
 */
export async function checkHosts(db: Kysely<DB>, deps: HostCheckDeps, userId: string, credentialId: string, modelId: string): Promise<OpenRouterHostAccess | null> {
  const row = await findCredential(db, userId, credentialId);
  const hosts = row?.kind === 'openrouter' ? ((await deps.routing?.hosts(modelId)) ?? null) : null;
  if (!row || !hosts || hosts.length === 0 || !deps.prober) {
    return null;
  }
  const credential = resolveCredential(deps.secretBox, row);
  if (!credential.apiKey) {
    return null;
  }
  const baseUrl = (row.baseUrl ?? PROVIDERS.openrouter.defaultBaseUrl ?? '').replace(/\/+$/, '');
  const states = await deps.prober.check({ id: row.id, apiKey: credential.apiKey, baseUrl }, modelId, hosts.map((host) => host.slug));
  return saveAccess(db, credentialId, modelId, states);
}

/**
 * The settings card: whether checks run, and what they found for the
 * models of the latest chats.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @returns The status.
 */
export async function hostCheckStatus(db: Kysely<DB>, userId: string): Promise<HostCheckStatus> {
  const enabled = await hostCheckEnabled(db, userId);
  const models = await Promise.all(
    (await recentOpenRouterModels(db, userId)).map(async (entry) => {
      const stored = enabled ? await storedAccess(db, entry.credentialId, entry.modelId) : null;
      return {
        model: entry.modelId,
        credentialId: entry.credentialId,
        credential: entry.credential,
        checkedAt: stored?.checkedAt ?? null,
        blocked: stored?.blocked.length ?? 0,
        allowed: stored?.allowed.length ?? 0,
      };
    }),
  );
  return { enabled, models };
}
