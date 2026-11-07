import { type OpenRouterHosts, pinOpenRouterHostSchema } from '@harness/shared';
import { Hono } from 'hono';
import { z } from 'zod';
import { HttpError } from '../http/errors.ts';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { routableModel } from '../openrouter.ts';
import { pinHost, pinnedHost } from '../services/hosts.ts';

const hostsQuerySchema = z.object({ model: z.string().max(200) });

/**
 * The hosts OpenRouter runs a model on, and the one the signed-in user
 * pinned it to.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/openrouter`.
 */
export function openRouterRoutes(deps: AppDeps) {
  const { db } = deps.database;

  async function answer(userId: string, model: string): Promise<OpenRouterHosts> {
    if (!routableModel(model)) {
      return { pinned: null, hosts: null };
    }
    const [pinned, hosts] = await Promise.all([pinnedHost(db, userId, model), deps.openRouter?.hosts(model) ?? null]);
    return { pinned, hosts };
  }

  return new Hono<AppEnv>()
    .get('/hosts', async (c) => {
      const { model } = hostsQuerySchema.parse(c.req.query());
      return c.json(await answer(c.get('user').id, model));
    })
    .put('/hosts', async (c) => {
      const { model, host } = pinOpenRouterHostSchema.parse(await c.req.json());
      const userId = c.get('user').id;
      if (host !== null) {
        // Without the list nothing can be checked; OpenRouter rejects a slug it does not know.
        const hosts = (await deps.openRouter?.hosts(model)) ?? null;
        if (hosts !== null && !hosts.some((entry) => entry.slug === host)) {
          throw new HttpError(400, 'validation', `OpenRouter runs ${model} on no host named ${host}`);
        }
      }
      await pinHost(db, userId, model, host);
      return c.json(await answer(userId, model));
    });
}
