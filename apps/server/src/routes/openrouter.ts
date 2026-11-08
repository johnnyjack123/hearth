import { type OpenRouterHostAccess, type OpenRouterHosts, pinOpenRouterHostSchema, PROVIDERS } from '@harness/shared';
import { Hono } from 'hono';
import { z } from 'zod';
import { HttpError, notFound } from '../http/errors.ts';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { routableModel } from '../openrouter.ts';
import { findCredential, resolveCredential } from '../services/credentials.ts';
import { pinHost, pinnedHost } from '../services/hosts.ts';

const hostsQuerySchema = z.object({ model: z.string().max(200) });
const accessQuerySchema = z.object({ model: z.string().max(200), credential: z.string().min(1).max(100) });

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
    // Asked separately: probing takes seconds, the list itself does not.
    .get('/access', async (c) => {
      const { model, credential: credentialId } = accessQuerySchema.parse(c.req.query());
      const row = await findCredential(db, c.get('user').id, credentialId);
      if (!row || row.kind !== 'openrouter') {
        throw notFound('OpenRouter credential');
      }
      const none: OpenRouterHostAccess = { blocked: [], allowed: [], checked: false };
      const hosts = routableModel(model) ? ((await deps.openRouter?.hosts(model)) ?? null) : null;
      const credential = resolveCredential(deps.secretBox, row);
      if (hosts === null || hosts.length === 0 || !credential.apiKey || !deps.hostAccess) {
        return c.json(none);
      }
      const states = await deps.hostAccess.check(
        { id: row.id, apiKey: credential.apiKey, baseUrl: (row.baseUrl ?? PROVIDERS.openrouter.defaultBaseUrl ?? '').replace(/\/+$/, '') },
        model,
        hosts.map((host) => host.slug),
      );
      const body: OpenRouterHostAccess = {
        blocked: [...states].filter(([, state]) => state === 'blocked').map(([slug]) => slug),
        allowed: [...states].filter(([, state]) => state === 'allowed').map(([slug]) => slug),
        checked: true,
      };
      return c.json(body);
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
