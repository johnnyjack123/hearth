import type { ServerActivity, UserActivity } from '@hearth/shared';
import { Hono } from 'hono';
import { type AppDeps, type AppEnv, requireAdmin } from '../http/context.ts';

/**
 * Who uses the server right now (`/api/admin/activity`, admins only): the
 * last request of each user in the web app, the replies being written for
 * them and their connected computers. All of it lives in memory, so it
 * starts empty after a restart.
 *
 * @param deps - Application dependencies.
 * @returns The router.
 */
export function adminActivityRoutes(deps: AppDeps) {
  const { db } = deps.database;
  return new Hono<AppEnv>().use(requireAdmin()).get('/', async (c) => {
    const running = deps.streams?.running() ?? [];
    const owners =
      running.length > 0 ? await db.selectFrom('conversation').select(['id', 'userId']).where('id', 'in', running).execute() : [];
    const users = new Map<string, UserActivity>();
    const entry = (userId: string) => {
      const found = users.get(userId) ?? { userId, lastSeenAt: deps.activity?.lastSeen(userId) ?? null, replies: 0, devices: 0 };
      users.set(userId, found);
      return found;
    };
    for (const userId of deps.activity?.users() ?? []) {
      entry(userId);
    }
    for (const owner of owners) {
      entry(owner.userId).replies += 1;
    }
    for (const device of deps.hub.online()) {
      entry(device.userId).devices += 1;
    }
    const body: ServerActivity = { since: deps.activity?.since ?? Date.now(), users: [...users.values()] };
    return c.json(body);
  });
}
