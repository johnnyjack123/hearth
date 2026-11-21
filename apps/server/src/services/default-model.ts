import { type DefaultModel, defaultModelSchema } from '@hearth/shared';
import type { Kysely } from 'kysely';
import type { DB } from '../db/schema.ts';
import { notFound } from '../http/errors.ts';

/** Key of the choice in the `userSetting` table. */
const KEY = 'defaultModel';

/**
 * The model the user's new chats start with. Null when they chose none, or
 * when the credential is gone: the browser then falls back to its last
 * choice. Whether the credential still offers the model, only its model
 * list knows; the web app checks that.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @returns The model, or null.
 */
export async function defaultModel(db: Kysely<DB>, userId: string): Promise<DefaultModel> {
  const row = await db.selectFrom('userSetting').select('value').where('userId', '=', userId).where('key', '=', KEY).executeTakeFirst();
  if (!row) {
    return null;
  }
  const parsed = defaultModelSchema.safeParse(JSON.parse(row.value));
  if (!parsed.success || !parsed.data) {
    return null;
  }
  const credential = await db
    .selectFrom('providerCredential')
    .select('id')
    .where('id', '=', parsed.data.credentialId)
    .where('userId', '=', userId)
    .executeTakeFirst();
  return credential ? parsed.data : null;
}

/**
 * Stores the model the user's new chats start with, or removes it.
 *
 * @param db - Open database.
 * @param userId - The user.
 * @param model - The model, or null for none.
 * @returns The stored choice.
 * @throws {HttpError} 404 when the credential is not the user's.
 */
export async function setDefaultModel(db: Kysely<DB>, userId: string, model: DefaultModel): Promise<DefaultModel> {
  if (model) {
    const credential = await db
      .selectFrom('providerCredential')
      .select('id')
      .where('id', '=', model.credentialId)
      .where('userId', '=', userId)
      .executeTakeFirst();
    if (!credential) {
      throw notFound('Credential');
    }
  }
  await db.transaction().execute(async (trx) => {
    await trx.deleteFrom('userSetting').where('userId', '=', userId).where('key', '=', KEY).execute();
    if (model) {
      await trx.insertInto('userSetting').values({ userId, key: KEY, value: JSON.stringify(model), updatedAt: Date.now() }).execute();
    }
  });
  return model;
}
