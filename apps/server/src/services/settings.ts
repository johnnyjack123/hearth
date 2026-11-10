import {
  type CompactSettings,
  type CondenseSettings,
  KEYED_SEARCH_PROVIDERS,
  type KeyedSearchProvider,
  type SearchSettings,
  type ServerSettings,
  type UpdateSearchSettings,
} from '@hearth/shared';
import type { Kysely } from 'kysely';
import { keyHint, type SecretBox } from '../crypto.ts';
import type { DB } from '../db/schema.ts';
import { HttpError } from '../http/errors.ts';
import {
  braveBackend,
  DEFAULT_SEARCH_ENDPOINTS,
  type SearchBackend,
  type SearchEndpoints,
  searxngBackend,
  tavilyBackend,
} from '../websearch/providers.ts';

const DEFAULTS: ServerSettings = {
  // Closed unless an admin opens it (the user's wish, 10.11.): the server
  // may stand on the internet, and the first account is never blocked.
  registration: 'closed',
  allowCustomBaseUrls: false,
  dailyCostLimit: null,
  search: {
    provider: 'off',
    searxngUrl: null,
    keys: { brave: null, tavily: null },
    helpers: { enabled: true, jevModel: 'typesafe/jev-1.13', embeddingModel: 'qwen/qwen3-embedding-8b' },
  },
  // 200,000 cached tokens cost DeepSeek users ~0.06 cents per model call;
  // half the window guards models with 128k. Six hours: DeepSeek keeps its
  // cache for hours, OpenAI and Anthropic for minutes.
  condense: { enabled: true, atTokens: 200_000, atWindowShare: 0.5, afterIdleHours: 6 },
  // As pi does it: summarize once fewer than 16,384 tokens of the window are
  // left, and keep the latest 20,000 tokens of the chat word for word.
  compact: { enabled: true, reserveTokens: 16_384, keepRecentTokens: 20_000 },
};

type SettingKey = keyof ServerSettings;

/** Setting rows of the encrypted search API keys; never part of {@link ServerSettings}. */
function searchKeyRow(provider: KeyedSearchProvider): string {
  return `searchKey.${provider}`;
}

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
  // Stored before a field existed: the defaults fill it in.
  settings.search = { ...DEFAULTS.search, ...settings.search, helpers: { ...DEFAULTS.search.helpers, ...settings.search.helpers } };
  settings.condense = { ...DEFAULTS.condense, ...settings.condense };
  settings.compact = { ...DEFAULTS.compact, ...settings.compact };
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
  patch: Omit<Partial<ServerSettings>, 'condense' | 'compact'> & {
    condense?: Partial<CondenseSettings>;
    compact?: Partial<CompactSettings>;
  },
): Promise<ServerSettings> {
  // A partial change keeps the other thresholds.
  const current = patch.condense || patch.compact ? await getServerSettings(db) : null;
  const merged = {
    ...patch,
    ...(current && patch.condense ? { condense: { ...current.condense, ...patch.condense } } : {}),
    ...(current && patch.compact ? { compact: { ...current.compact, ...patch.compact } } : {}),
  };
  const entries = Object.entries(merged) as [SettingKey, ServerSettings[SettingKey]][];
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
 * Changes the web search settings. Keys are stored encrypted, bound to their
 * row; the settings only keep a hint of them.
 *
 * @param db - Open database.
 * @param secretBox - Encryption for the keys.
 * @param patch - Validated change; a key of null deletes the stored one.
 * @returns The effective settings after the update.
 * @throws {HttpError} 400 if the chosen provider lacks its URL or key.
 */
export async function updateSearchSettings(
  db: Kysely<DB>,
  secretBox: SecretBox,
  patch: UpdateSearchSettings,
): Promise<ServerSettings> {
  const current = (await getServerSettings(db)).search;
  const next: SearchSettings = {
    provider: patch.provider ?? current.provider,
    searxngUrl: patch.searxngUrl === undefined ? current.searxngUrl : patch.searxngUrl,
    keys: { ...current.keys },
    helpers: { ...current.helpers, ...patch.helpers },
  };
  const keys = { brave: patch.braveKey, tavily: patch.tavilyKey };
  for (const provider of KEYED_SEARCH_PROVIDERS) {
    const key = keys[provider];
    if (key !== undefined) {
      next.keys[provider] = key === null ? null : keyHint(key);
    }
  }
  if (next.provider === 'searxng' && !next.searxngUrl) {
    throw new HttpError(400, 'validation', 'SearXNG needs the URL of the instance');
  }
  if ((next.provider === 'brave' || next.provider === 'tavily') && !next.keys[next.provider]) {
    throw new HttpError(400, 'validation', `${next.provider} needs an API key`);
  }

  await db.transaction().execute(async (trx) => {
    for (const provider of KEYED_SEARCH_PROVIDERS) {
      const key = keys[provider];
      const row = searchKeyRow(provider);
      if (key === null) {
        await trx.deleteFrom('setting').where('key', '=', row).execute();
      } else if (key !== undefined) {
        const value = JSON.stringify(secretBox.encrypt(key, `setting:${row}`));
        await trx
          .insertInto('setting')
          .values({ key: row, value })
          .onConflict((oc) => oc.column('key').doUpdateSet({ value }))
          .execute();
      }
    }
    const value = JSON.stringify(next);
    await trx
      .insertInto('setting')
      .values({ key: 'search', value })
      .onConflict((oc) => oc.column('key').doUpdateSet({ value }))
      .execute();
  });
  return getServerSettings(db);
}

/**
 * The search provider of the server settings, ready to search.
 *
 * @param db - Open database.
 * @param secretBox - Decrypts the stored key.
 * @param endpoints - Endpoints and time limit; tests replace them.
 * @returns The backend, or null if search is off or its key unreadable
 *   (e.g. after the server secret changed).
 */
export async function loadSearchBackend(
  db: Kysely<DB>,
  secretBox: SecretBox,
  endpoints: SearchEndpoints = DEFAULT_SEARCH_ENDPOINTS,
): Promise<SearchBackend | null> {
  const { search } = await getServerSettings(db);
  if (search.provider === 'off') {
    return null;
  }
  if (search.provider === 'searxng') {
    return search.searxngUrl ? searxngBackend(search.searxngUrl, endpoints) : null;
  }
  const provider = search.provider;
  const row = await db.selectFrom('setting').select('value').where('key', '=', searchKeyRow(provider)).executeTakeFirst();
  if (!row) {
    return null;
  }
  let key: string;
  try {
    key = secretBox.decrypt(JSON.parse(row.value) as string, `setting:${searchKeyRow(provider)}`);
  } catch {
    console.warn(`[search] the stored ${provider} key cannot be decrypted; enter it again in the server settings`);
    return null;
  }
  return provider === 'brave' ? braveBackend(key, endpoints) : tavilyBackend(key, endpoints);
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
