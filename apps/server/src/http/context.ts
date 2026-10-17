import { createMiddleware } from 'hono/factory';
import type { Kysely } from 'kysely';
import type { Auth } from '../auth.ts';
import type { ServerConfig } from '../config.ts';
import type { SecretBox } from '../crypto.ts';
import type { Database } from '../db/index.ts';
import type { DB } from '../db/schema.ts';
import type { LlmGateway } from '../llm.ts';
import type { DeviceHub } from '../tunnel/hub.ts';
import { authenticateDevice, type DeviceOwner, type DeviceRow } from '../services/devices.ts';
import { HttpError } from './errors.ts';

/**
 * Everything the HTTP layer needs; created once at startup and injected so
 * tests can substitute parts (notably the LLM gateway).
 */
export interface AppDeps {
  /** Server configuration. */
  config: ServerConfig;
  /** Open database. */
  database: Database;
  /** Auth instance. */
  auth: Auth;
  /** Encryption for stored API keys. */
  secretBox: SecretBox;
  /** Access to LLM providers. */
  llm: LlmGateway;
  /** Connected devices and the agent replies running on them. */
  hub: DeviceHub;
}

/**
 * Signed-in user as provided by Better Auth, including the admin plugin's role.
 */
export type SessionUser = Auth['$Infer']['Session']['user'];

/**
 * Hono environment shared by all routes.
 */
export interface AppEnv {
  /** Request-scoped values set by middleware. */
  Variables: {
    /** The signed-in user; guaranteed by {@link requireUser}. */
    user: SessionUser;
  };
}

/**
 * Resolves the session cookie and rejects anonymous requests with 401.
 *
 * @param auth - Auth instance used to read the session.
 * @returns The middleware.
 */
export function requireUser(auth: Auth) {
  return createMiddleware<AppEnv>(async (c, next) => {
    const session = await auth.api.getSession({ headers: c.req.raw.headers });
    if (!session) {
      throw new HttpError(401, 'unauthorized', 'Not signed in');
    }
    c.set('user', session.user);
    await next();
  });
}

/**
 * Hono environment of routes called by paired devices.
 */
export interface DeviceEnv {
  /** Request-scoped values set by {@link requireDevice}. */
  Variables: {
    /** The calling device. */
    device: DeviceRow;
    /** The account the device acts for. */
    owner: DeviceOwner;
  };
}

/**
 * Resolves `Authorization: Bearer <device token>` and rejects other requests
 * with 401. Device tokens are only accepted by routes that use this
 * middleware, never by {@link requireUser}, so a leaked token gives LLM access
 * but not the account's chats or keys.
 *
 * @param db - Open database.
 * @returns The middleware.
 */
export function requireDevice(db: Kysely<DB>) {
  return createMiddleware<DeviceEnv>(async (c, next) => {
    const header = c.req.header('authorization') ?? '';
    const token = header.startsWith('Bearer ') ? header.slice('Bearer '.length).trim() : '';
    const found = token ? await authenticateDevice(db, token) : undefined;
    if (!found) {
      throw new HttpError(401, 'unauthorized', 'Unknown or revoked device token');
    }
    c.set('device', found.device);
    c.set('owner', found.owner);
    await next();
  });
}

/**
 * Rejects requests from users without the admin role with 403. Must run after
 * {@link requireUser}.
 *
 * @returns The middleware.
 */
export function requireAdmin() {
  return createMiddleware<AppEnv>(async (c, next) => {
    if (!isAdmin(c.get('user'))) {
      throw new HttpError(403, 'forbidden', 'Admin role required');
    }
    await next();
  });
}

/**
 * Checks for the admin role. The admin plugin stores roles comma-separated.
 *
 * @param user - Signed-in user or a device's owner.
 * @returns True if one of the user's roles is `admin`.
 */
export function isAdmin(user: { role?: string | null }): boolean {
  return (user.role ?? '').split(',').map((role) => role.trim()).includes('admin');
}
