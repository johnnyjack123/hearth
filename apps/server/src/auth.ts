import { type BetterAuthOptions, betterAuth } from 'better-auth';
import { APIError } from 'better-auth/api';
import { getMigrations } from 'better-auth/db/migration';
import { admin } from 'better-auth/plugins';
import type { ServerConfig } from './config.ts';
import { deriveKey } from './crypto.ts';
import type { Database } from './db/index.ts';
import { countUsers, getServerSettings } from './services/settings.ts';

/**
 * Header the server sets to the client address it determined (see
 * `http/client-ip.ts`); Better Auth reads only this one, never a
 * client's own `X-Forwarded-For`.
 */
export const CLIENT_IP_HEADER = 'x-harness-client-ip';

function authOptions(config: ServerConfig, database: Database) {
  return {
    appName: 'LLM Harness',
    baseURL: config.publicUrl,
    basePath: '/api/auth',
    secret: deriveKey(config.secret, 'auth:v1').toString('base64url'),
    trustedOrigins: config.trustedOrigins,
    database: { db: database.db, type: database.kind },
    emailAndPassword: {
      enabled: true,
      minPasswordLength: 10,
      maxPasswordLength: 256,
      autoSignIn: true,
    },
    plugins: [admin()],
    telemetry: { enabled: false },
    // On in production (Better Auth's default), per client address. Stricter
    // than the defaults (3 in 10 s): a password guesser gets 5 a minute.
    rateLimit: {
      customRules: {
        '/sign-in/email': { window: 60, max: 5 },
        '/sign-up/email': { window: 600, max: 5 },
        '/change-password': { window: 60, max: 5 },
      },
    },
    advanced: {
      cookiePrefix: 'harness',
      useSecureCookies: config.publicUrl.startsWith('https://'),
      ipAddress: { ipAddressHeaders: [CLIENT_IP_HEADER] },
    },
    databaseHooks: {
      user: {
        create: {
          before: async (user, ctx) => {
            if ((await countUsers(database.db)) === 0) {
              return { data: { ...user, role: 'admin' } };
            }
            // The admin plugin authorizes this endpoint itself.
            if (ctx?.path === '/admin/create-user') {
              return;
            }
            const settings = await getServerSettings(database.db);
            if (settings.registration === 'closed') {
              throw new APIError('FORBIDDEN', {
                message: 'Registration is closed',
                code: 'REGISTRATION_CLOSED',
              });
            }
          },
        },
      },
    },
  } satisfies BetterAuthOptions;
}

/**
 * Creates the Better Auth instance: email/password accounts, admin roles and
 * the registration policy. Call {@link migrateAuth} first, otherwise Better
 * Auth's startup schema check reports missing tables.
 *
 * The first account ever created becomes admin. Afterwards, sign-up follows
 * the `registration` setting; admins can always create accounts through the
 * admin API.
 *
 * @param config - Server configuration.
 * @param database - Open database shared with the app.
 * @returns The auth instance.
 */
export function createAuth(config: ServerConfig, database: Database) {
  return betterAuth(authOptions(config, database));
}

/**
 * The configured Better Auth instance.
 */
export type Auth = ReturnType<typeof createAuth>;

/**
 * Creates or updates Better Auth's own tables.
 *
 * @param config - Server configuration.
 * @param database - Open database.
 */
export async function migrateAuth(config: ServerConfig, database: Database): Promise<void> {
  const { runMigrations } = await getMigrations(authOptions(config, database));
  await runMigrations();
}
