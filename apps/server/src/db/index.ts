import { mkdirSync } from 'node:fs';
import path from 'node:path';
import BetterSqlite3 from 'better-sqlite3';
import { Kysely, PostgresDialect, SqliteDialect } from 'kysely';
import pg from 'pg';
import type { DB } from './schema.ts';

/**
 * SQL dialect in use. Matches Better Auth's `KyselyDatabaseType` values.
 */
export type DatabaseKind = 'sqlite' | 'postgres';

/**
 * An open database connection with its dialect.
 */
export interface Database {
  /** Query builder for the app's tables. */
  db: Kysely<DB>;
  /** Dialect, needed by Better Auth and for the few dialect-specific spots. */
  kind: DatabaseKind;
  /** Closes the connection or pool. */
  close: () => Promise<void>;
}

/**
 * Parsed form of a `DATABASE_URL`.
 */
export type DatabaseTarget = { kind: 'sqlite'; file: string } | { kind: 'postgres'; url: string };

/**
 * Parses a database URL.
 *
 * Accepted forms: `postgres://…`, `postgresql://…`, `sqlite:<path>`,
 * `file:<path>` and `sqlite::memory:` for an in-memory database.
 *
 * @param url - Connection string from the configuration.
 * @returns The dialect and its connection target.
 * @throws {Error} If the scheme is not supported.
 */
export function parseDatabaseUrl(url: string): DatabaseTarget {
  if (/^postgres(ql)?:\/\//i.test(url)) {
    return { kind: 'postgres', url };
  }
  const sqliteMatch = /^(?:sqlite|file):(.+)$/i.exec(url);
  if (sqliteMatch?.[1]) {
    const file = sqliteMatch[1].replace(/^\/\/(?=[a-z]:|\/)/i, '');
    return { kind: 'sqlite', file };
  }
  throw new Error(`Unsupported DATABASE_URL scheme: ${url.split(':')[0]}`);
}

// node-postgres returns BIGINT as string to avoid precision loss. Our BIGINT
// columns only hold epoch milliseconds, which fit safely into a JS number.
pg.types.setTypeParser(pg.types.builtins.INT8, (value) => Number(value));

/**
 * Opens a database for the given URL.
 *
 * @param url - Connection string, see {@link parseDatabaseUrl}.
 * @returns The open database. Postgres connects lazily on the first query.
 */
export function createDatabase(url: string): Database {
  const target = parseDatabaseUrl(url);

  if (target.kind === 'postgres') {
    const pool = new pg.Pool({ connectionString: target.url, max: 10 });
    const db = new Kysely<DB>({ dialect: new PostgresDialect({ pool }) });
    return { db, kind: 'postgres', close: () => db.destroy() };
  }

  if (target.file !== ':memory:') {
    mkdirSync(path.dirname(path.resolve(target.file)), { recursive: true });
  }
  const sqlite = new BetterSqlite3(target.file);
  sqlite.pragma('journal_mode = WAL');
  // SQLite ignores foreign keys unless enabled per connection; cascades depend on it.
  sqlite.pragma('foreign_keys = ON');
  sqlite.pragma('busy_timeout = 5000');
  const db = new Kysely<DB>({ dialect: new SqliteDialect({ database: sqlite }) });
  return { db, kind: 'sqlite', close: () => db.destroy() };
}
