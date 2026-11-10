import { randomBytes, randomInt } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { z } from 'zod';
import { parseTrustedProxies, type TrustedProxies } from './http/client-ip.ts';

const envSchema = z.object({
  HOST: z.string().default('127.0.0.1'),
  PORT: z.coerce.number().int().min(1).max(65535).default(8080),
  PUBLIC_URL: z.url().default('http://localhost:8080'),
  DATA_DIR: z.string().default('./data'),
  DATABASE_URL: z.string().optional(),
  HEARTH_SECRET: z.string().min(32, 'HEARTH_SECRET must be at least 32 characters').optional(),
  TRUSTED_ORIGINS: z.string().optional(),
  WEB_DIST: z.string().optional(),
  MARKITDOWN_URL: z.url().optional(),
  TRUSTED_PROXIES: z.string().optional(),
  HEARTH_SETUP_CODE: z.string().min(8, 'HEARTH_SETUP_CODE must be at least 8 characters').optional(),
});

/**
 * Fully resolved server configuration.
 */
export interface ServerConfig {
  /** Interface to bind to. */
  host: string;
  /** TCP port to listen on. */
  port: number;
  /** Externally visible origin, used for auth callbacks and cookie security. */
  publicUrl: string;
  /** Directory for the SQLite file and the generated secret. */
  dataDir: string;
  /** `sqlite:<path>` or a `postgres://` connection string. */
  databaseUrl: string;
  /** Root secret from which auth and encryption keys are derived. */
  secret: string;
  /** Origins allowed to make authenticated requests, including `publicUrl`. */
  trustedOrigins: string[];
  /** Directory of the built web app, or null to serve the API only. */
  webDist: string | null;
  /**
   * Document converter (apps/markitdown), e.g. `http://markitdown:8080`;
   * without it, chats take images and text files but no documents.
   */
  markitdownUrl?: string | null;
  /**
   * Reverse proxies whose `X-Forwarded-For` names the client (`TRUSTED_PROXIES`);
   * none when unset, then the address of the connection counts.
   */
  trustedProxies?: TrustedProxies;
  /**
   * Code the first account must give at sign-up, printed in the log while
   * the server has no account; none needed when unset (tests).
   */
  setupCode?: string;
}

const SECRET_FILE = 'server-secret';

/**
 * Variables the server read before it was renamed (llm-harness → hearth,
 * 0.2.0). A `.env` that still names them keeps working; above all
 * `HARNESS_SECRET`, without which the stored API keys could not be read.
 *
 * @param env - The environment, empty values removed.
 * @returns The old values under their new names.
 */
function legacyVariables(env: Record<string, string | undefined>): Record<string, string> {
  const renamed: Record<string, string> = {};
  for (const name of ['SECRET', 'SETUP_CODE']) {
    const old = env[`HARNESS_${name}`];
    if (old !== undefined) {
      renamed[`HEARTH_${name}`] = old;
    }
  }
  return renamed;
}

/**
 * The SQLite file without `DATABASE_URL`: `hearth.db`, or `harness.db` of a
 * server from before the rename, so its accounts and chats stay.
 *
 * @param dataDir - The data directory.
 * @returns The file's path.
 */
function sqliteFile(dataDir: string): string {
  const current = path.join(dataDir, 'hearth.db');
  const legacy = path.join(dataDir, 'harness.db');
  return !existsSync(current) && existsSync(legacy) ? legacy : current;
}

/** Shortest secret the server accepts, from the file as from `HEARTH_SECRET`. */
const MIN_SECRET_LENGTH = 32;

// Without 0/O and 1/I, like the pairing codes: read off a log, typed by hand.
const SETUP_CODE_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';

/**
 * A random code for the first account, formatted `ABCD-EFGH`.
 *
 * @returns The code.
 */
function newSetupCode(): string {
  const chars = Array.from({ length: 8 }, () => SETUP_CODE_ALPHABET[randomInt(SETUP_CODE_ALPHABET.length)]);
  return `${chars.slice(0, 4).join('')}-${chars.slice(4).join('')}`;
}

/**
 * A hint for start failures with a known cause in the Docker setup.
 *
 * @param error - What made the start fail.
 * @returns One line for the log, or null.
 */
export function startupHint(error: unknown): string | null {
  const { code, hostname } = (error ?? {}) as { code?: unknown; hostname?: unknown };
  // The compose file starts Postgres only with its profile; without it the
  // service name does not resolve.
  if (code === 'ENOTFOUND' && hostname === 'postgres') {
    return (
      'The host "postgres" does not resolve: the Postgres container is not running. With docker compose, ' +
      'set COMPOSE_PROFILES=postgres in .env (or run docker compose --profile postgres up -d).'
    );
  }
  return null;
}

/**
 * Rejects a Postgres URL that the driver could not parse, with a hint
 * instead of pg's bare "Invalid URL". Passwords from `openssl rand -base64`
 * contain `/` and `+`, which break the URL unless percent-encoded. The
 * message never repeats the URL, it contains the password.
 *
 * @param url - Value of `DATABASE_URL`.
 * @throws {Error} If it is a Postgres URL that is not a valid URL.
 */
export function checkDatabaseUrl(url: string): void {
  if (/^postgres(ql)?:\/\//i.test(url) && !URL.canParse(url)) {
    throw new Error(
      'DATABASE_URL is not a valid URL. Characters like / ? # @ : % in the password must be percent-encoded ' +
        '(/ as %2F, + as %2B, = as %3D), or use a password of letters and digits only (openssl rand -hex 32).',
    );
  }
}

/**
 * Reads the server configuration from environment variables.
 *
 * Without `HEARTH_SECRET`, a random secret is generated once and stored in the
 * data directory, so a fresh install works without any configuration. Losing
 * that file invalidates sessions and makes stored API keys unreadable.
 *
 * @param env - Environment to read, defaults to `process.env`.
 * @returns The resolved configuration.
 * @throws {z.ZodError} If a variable is present but invalid.
 */
export function loadConfig(env: NodeJS.ProcessEnv = process.env): ServerConfig {
  // Docker Compose passes unset `${VAR:-}` as an empty string; empty means
  // "not set", never an empty database URL or secret.
  const given = Object.fromEntries(Object.entries(env).filter(([, value]) => value !== ''));
  const parsed = envSchema.parse({ ...legacyVariables(given), ...given });
  if (parsed.DATABASE_URL) {
    checkDatabaseUrl(parsed.DATABASE_URL);
  }
  const dataDir = path.resolve(parsed.DATA_DIR);
  mkdirSync(dataDir, { recursive: true });

  const publicUrl = parsed.PUBLIC_URL.replace(/\/+$/, '');
  const extraOrigins = (parsed.TRUSTED_ORIGINS ?? '')
    .split(',')
    .map((origin) => origin.trim().replace(/\/+$/, ''))
    .filter(Boolean);
  if (env.NODE_ENV !== 'production') {
    // The Vite dev server proxies /api, so requests carry its origin.
    extraOrigins.push('http://localhost:5173');
  }

  return {
    host: parsed.HOST,
    port: parsed.PORT,
    publicUrl,
    dataDir,
    databaseUrl: parsed.DATABASE_URL ?? `sqlite:${sqliteFile(dataDir)}`,
    secret: parsed.HEARTH_SECRET ?? readOrCreateSecret(dataDir),
    trustedOrigins: [...new Set([new URL(publicUrl).origin, ...extraOrigins])],
    webDist: resolveWebDist(parsed.WEB_DIST),
    markitdownUrl: parsed.MARKITDOWN_URL?.replace(/\/+$/, '') ?? null,
    trustedProxies: parseTrustedProxies(parsed.TRUSTED_PROXIES),
    setupCode: parsed.HEARTH_SETUP_CODE ?? newSetupCode(),
  };
}

function readOrCreateSecret(dataDir: string): string {
  const file = path.join(dataDir, SECRET_FILE);
  if (existsSync(file)) {
    const stored = readFileSync(file, 'utf8').trim();
    // An empty or truncated file would make every key derivable from little.
    if (stored.length < MIN_SECRET_LENGTH) {
      throw new Error(
        `${file} holds only ${stored.length} characters, at least ${MIN_SECRET_LENGTH} are needed. ` +
          'Restore the file from a backup (stored API keys are bound to it), or remove it to start over without them.',
      );
    }
    return stored;
  }
  const secret = randomBytes(48).toString('base64url');
  try {
    writeFileSync(file, `${secret}\n`, { mode: 0o600 });
  } catch (error) {
    // Typical with a bind mount that Docker created as root: the image runs
    // as uid 1000 and cannot write there.
    if ((error as NodeJS.ErrnoException).code === 'EACCES') {
      throw new Error(
        `Cannot write ${file}: the data directory is not writable for uid ${process.getuid?.() ?? '?'}. ` +
          'For a bind mount, give the folder to that user (chown 1000:1000 <folder>) or set HEARTH_SECRET.',
      );
    }
    throw error;
  }
  return secret;
}

function resolveWebDist(configured: string | undefined): string | null {
  if (configured) {
    return path.resolve(configured);
  }
  // In the monorepo the server runs from apps/server; serve the sibling build if present.
  const sibling = path.resolve('../web/dist');
  return existsSync(path.join(sibling, 'index.html')) ? sibling : null;
}
