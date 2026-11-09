import { type OpenRouterHostAccess, type OpenRouterHosts, pinOpenRouterHostSchema } from '@harness/shared';
import { Hono } from 'hono';
import { z } from 'zod';
import { HttpError, notFound } from '../http/errors.ts';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { routableModel } from '../openrouter.ts';
import { findCredential } from '../services/credentials.ts';
import {
  checkHosts,
  HOST_CHECK_MAX_AGE_MS,
  hostCheckEnabled,
  hostCheckStatus,
  recentOpenRouterModels,
  setHostCheck,
  storedAccess,
} from '../services/host-access.ts';
import { pinHost, pinnedHost } from '../services/hosts.ts';

const hostsQuerySchema = z.object({ model: z.string().max(200) });
const accessQuerySchema = z.object({ model: z.string().max(200), credential: z.string().min(1).max(100) });
const hostCheckSettingsSchema = z.object({ enabled: z.boolean() }).strict();

/**
 * The hosts OpenRouter runs a model on, the one the signed-in user pinned
 * it to, and which of them their account allows.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/openrouter`.
 */
export function openRouterRoutes(deps: AppDeps) {
  const { db } = deps.database;
  const checkDeps = { secretBox: deps.secretBox, routing: deps.openRouter, prober: deps.hostProber };

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
    // What the last check found. Only the models of the five latest chats
    // are checked, once a week (the user's wish: probes cost a little).
    .get('/access', async (c) => {
      const { model, credential } = accessQuerySchema.parse(c.req.query());
      const userId = c.get('user').id;
      const row = await findCredential(db, userId, credential);
      if (!row || row.kind !== 'openrouter') {
        throw notFound('OpenRouter credential');
      }
      const none: OpenRouterHostAccess = { blocked: [], allowed: [], checked: false, checkedAt: null };
      if (!routableModel(model)) {
        return c.json(none);
      }
      if (!(await hostCheckEnabled(db, userId))) {
        return c.json({ ...none, disabled: true } satisfies OpenRouterHostAccess);
      }
      const stored = await storedAccess(db, credential, model);
      const due = !stored || Date.now() - (stored.checkedAt ?? 0) >= HOST_CHECK_MAX_AGE_MS;
      const recent = (await recentOpenRouterModels(db, userId)).some((entry) => entry.credentialId === credential && entry.modelId === model);
      if (due && recent) {
        return c.json((await checkHosts(db, checkDeps, userId, credential, model)) ?? stored ?? none);
      }
      return c.json(stored ?? none);
    })
    .get('/access/status', async (c) => c.json(await hostCheckStatus(db, c.get('user').id)))
    // The button in the settings: all models of the latest chats, now.
    .post('/access/refresh', async (c) => {
      const userId = c.get('user').id;
      if (!(await hostCheckEnabled(db, userId))) {
        throw new HttpError(409, 'conflict', 'Host checks are switched off');
      }
      for (const entry of await recentOpenRouterModels(db, userId)) {
        await checkHosts(db, checkDeps, userId, entry.credentialId, entry.modelId);
      }
      return c.json(await hostCheckStatus(db, userId));
    })
    .put('/access/settings', async (c) => {
      const { enabled } = hostCheckSettingsSchema.parse(await c.req.json());
      const userId = c.get('user').id;
      await setHostCheck(db, userId, enabled);
      return c.json(await hostCheckStatus(db, userId));
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
