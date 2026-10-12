import { randomBytes } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { z } from 'zod';

const envSchema = z.object({
  HOST: z.string().default('127.0.0.1'),
  PORT: z.coerce.number().int().min(1).max(65535).default(8080),
  PUBLIC_URL: z.url().default('http://localhost:8080'),
  DATA_DIR: z.string().default('./data'),
  DATABASE_URL: z.string().optional(),
  HARNESS_SECRET: z.string().min(32, 'HARNESS_SECRET must be at least 32 characters').optional(),
  TRUSTED_ORIGINS: z.string().optional(),
  WEB_DIST: z.string().optional(),
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
}

const SECRET_FILE = 'server-secret';

/**
 * Reads the server configuration from environment variables.
 *
 * Without `HARNESS_SECRET`, a random secret is generated once and stored in the
 * data directory, so a fresh install works without any configuration. Losing
 * that file invalidates sessions and makes stored API keys unreadable.
 *
 * @param env - Environment to read, defaults to `process.env`.
 * @returns The resolved configuration.
 * @throws {z.ZodError} If a variable is present but invalid.
 */
export function loadConfig(env: NodeJS.ProcessEnv = process.env): ServerConfig {
  const parsed = envSchema.parse(env);
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
    databaseUrl: parsed.DATABASE_URL ?? `sqlite:${path.join(dataDir, 'harness.db')}`,
    secret: parsed.HARNESS_SECRET ?? readOrCreateSecret(dataDir),
    trustedOrigins: [...new Set([new URL(publicUrl).origin, ...extraOrigins])],
    webDist: resolveWebDist(parsed.WEB_DIST),
  };
}

function readOrCreateSecret(dataDir: string): string {
  const file = path.join(dataDir, SECRET_FILE);
  if (existsSync(file)) {
    return readFileSync(file, 'utf8').trim();
  }
  const secret = randomBytes(48).toString('base64url');
  writeFileSync(file, `${secret}\n`, { mode: 0o600 });
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
