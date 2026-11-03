import { updateCostLimitSchema } from '@harness/shared';
import { Hono } from 'hono';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { costGuard, setOwnLimit } from '../services/costs.ts';

/**
 * The daily cost limit of the signed-in user: what was spent today, and the
 * user's own limit.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/costs`.
 */
export function costRoutes(deps: AppDeps) {
  const { db } = deps.database;
  return new Hono<AppEnv>()
    .get('/', async (c) => c.json(await costGuard(db).status(c.get('user').id)))
    .put('/limit', async (c) => {
      const { daily } = updateCostLimitSchema.parse(await c.req.json());
      await setOwnLimit(db, c.get('user').id, daily);
      return c.json(await costGuard(db).status(c.get('user').id));
    });
}
