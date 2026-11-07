import type { Kysely } from 'kysely';
import type { DB } from '../db/schema.ts';

/**
 * The OpenRouter host a user pinned a model to: every call of the user's
 * chats and agents with this model goes there and nowhere else.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @param modelId - OpenRouter's model id.
 * @returns The provider's slug, or null to route to the cheapest.
 */
export async function pinnedHost(db: Kysely<DB>, userId: string, modelId: string): Promise<string | null> {
  const row = await db
    .selectFrom('openRouterPin')
    .select('host')
    .where('userId', '=', userId)
    .where('modelId', '=', modelId)
    .executeTakeFirst();
  return row?.host ?? null;
}

/**
 * Pins a model to a host, or with null lets it go to the cheapest again.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @param modelId - OpenRouter's model id.
 * @param host - The provider's slug, or null.
 */
export async function pinHost(db: Kysely<DB>, userId: string, modelId: string, host: string | null): Promise<void> {
  if (host === null) {
    await db.deleteFrom('openRouterPin').where('userId', '=', userId).where('modelId', '=', modelId).execute();
    return;
  }
  const updatedAt = Date.now();
  await db
    .insertInto('openRouterPin')
    .values({ userId, modelId, host, updatedAt })
    .onConflict((oc) => oc.columns(['userId', 'modelId']).doUpdateSet({ host, updatedAt }))
    .execute();
}
