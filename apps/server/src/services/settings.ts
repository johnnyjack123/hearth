import type { ServerSettings } from '@harness/shared';
import type { Kysely } from 'kysely';
import type { DB } from '../db/schema.ts';

const DEFAULTS: ServerSettings = {
  registration: 'open',
  allowCustomBaseUrls: false,
};

type SettingKey = keyof ServerSettings;

/**
 * Reads the server settings, falling back to defaults for unset keys.
 *
 * @param db - Open database.
 * @returns The effective settings.
 */
export async function getServerSettings(db: Kysely<DB>): Promise<ServerSettings> {
  const rows = await db.selectFrom('setting').select(['key', 'value']).execute();
  const settings: ServerSettings = { ...DEFAULTS };
  for (const row of rows) {
    if (row.key in DEFAULTS) {
      // Values were validated on write; unknown keys are ignored to tolerate downgrades.
      (settings as unknown as Record<string, unknown>)[row.key] = JSON.parse(row.value);
    }
  }
  return settings;
}

/**
 * Stores the given settings and returns the full, updated set.
 *
 * @param db - Open database.
 * @param patch - Validated subset of settings to change.
 * @returns The effective settings after the update.
 */
export async function updateServerSettings(
  db: Kysely<DB>,
  patch: Partial<ServerSettings>,
): Promise<ServerSettings> {
  const entries = Object.entries(patch) as [SettingKey, ServerSettings[SettingKey]][];
  for (const [key, value] of entries) {
    if (value === undefined) {
      continue;
    }
    const json = JSON.stringify(value);
    await db
      .insertInto('setting')
      .values({ key, value: json })
      .onConflict((oc) => oc.column('key').doUpdateSet({ value: json }))
      .execute();
  }
  return getServerSettings(db);
}

/**
 * Counts registered accounts.
 *
 * @param db - Open database.
 * @returns Number of rows in the Better Auth `user` table.
 */
export async function countUsers(db: Kysely<DB>): Promise<number> {
  const row = await db
    .selectFrom('user')
    .select((eb) => eb.fn.countAll<number | string>().as('count'))
    .executeTakeFirstOrThrow();
  return Number(row.count);
}
