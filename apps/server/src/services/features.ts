import { type ChatFeatures, DEFAULT_CHAT_FEATURES, type UpdateChatFeatures } from '@hearth/shared';
import type { Kysely } from 'kysely';
import type { DB } from '../db/schema.ts';

const FEATURES = Object.keys(DEFAULT_CHAT_FEATURES) as (keyof ChatFeatures)[];

/**
 * Stored values of features, by name, from rows of the `setting` or
 * `userSetting` table; anything that is not a boolean is left out.
 */
function booleans(rows: { key: string; value: string }[]): Partial<ChatFeatures> {
  const found: Partial<ChatFeatures> = {};
  for (const row of rows) {
    const value: unknown = JSON.parse(row.value);
    if ((FEATURES as string[]).includes(row.key) && typeof value === 'boolean') {
      found[row.key as keyof ChatFeatures] = value;
    }
  }
  return found;
}

/**
 * The chat features of a user: their own choice, else what an admin set for
 * the whole server before 0.1.22 (the features were server settings then),
 * else the default.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @returns The features.
 */
export async function chatFeatures(db: Kysely<DB>, userId: string): Promise<ChatFeatures> {
  const [server, own] = await Promise.all([
    db.selectFrom('setting').select(['key', 'value']).where('key', 'in', FEATURES).execute(),
    db.selectFrom('userSetting').select(['key', 'value']).where('userId', '=', userId).where('key', 'in', FEATURES).execute(),
  ]);
  return { ...DEFAULT_CHAT_FEATURES, ...booleans(server), ...booleans(own) };
}

/**
 * Stores some of a user's chat features.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @param patch - The features to change.
 * @returns All features after the change.
 */
export async function setChatFeatures(db: Kysely<DB>, userId: string, patch: UpdateChatFeatures): Promise<ChatFeatures> {
  const changes = Object.entries(patch).filter(([, value]) => value !== undefined);
  if (changes.length > 0) {
    const now = Date.now();
    await db.transaction().execute(async (trx) => {
      const keys = changes.map(([key]) => key);
      await trx.deleteFrom('userSetting').where('userId', '=', userId).where('key', 'in', keys).execute();
      await trx
        .insertInto('userSetting')
        .values(changes.map(([key, value]) => ({ userId, key, value: JSON.stringify(value), updatedAt: now })))
        .execute();
    });
  }
  return chatFeatures(db, userId);
}
