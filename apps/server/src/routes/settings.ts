import { type PublicSettings, updateServerSettingsSchema } from '@harness/shared';
import { Hono } from 'hono';
import { type AppDeps, type AppEnv, requireAdmin } from '../http/context.ts';
import { countUsers, getServerSettings, updateServerSettings } from '../services/settings.ts';

/**
 * Public settings, readable without a session (mounted at `/api/settings`).
 *
 * @param deps - Application dependencies.
 * @returns The router.
 */
export function publicSettingsRoutes(deps: AppDeps) {
  const { db } = deps.database;
  return new Hono().get('/', async (c) => {
    const settings = await getServerSettings(db);
    const body: PublicSettings = {
      registration: settings.registration,
      hasUsers: (await countUsers(db)) > 0,
      allowCustomBaseUrls: settings.allowCustomBaseUrls,
    };
    return c.json(body);
  });
}

/**
 * Admin-only server settings (mounted at `/api/admin/settings`). User
 * management itself is served by the Better Auth admin plugin.
 *
 * @param deps - Application dependencies.
 * @returns The router.
 */
export function adminSettingsRoutes(deps: AppDeps) {
  const { db } = deps.database;
  return new Hono<AppEnv>()
    .use(requireAdmin())
    .get('/', async (c) => c.json(await getServerSettings(db)))
    .patch('/', async (c) => {
      const patch = updateServerSettingsSchema.parse(await c.req.json());
      return c.json(await updateServerSettings(db, patch));
    });
}
