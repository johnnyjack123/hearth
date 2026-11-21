import { defaultModelSchema, updateChatFeaturesSchema } from '@hearth/shared';
import { Hono } from 'hono';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { defaultModel, setDefaultModel } from '../services/default-model.ts';
import { chatFeatures, setChatFeatures } from '../services/features.ts';

/**
 * The signed-in user's chat features (`/api/features`): which tools their
 * chats and agents get, how OpenRouter picks hosts for them, and the model
 * new chats start with (`/api/features/default-model`).
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/features`.
 */
export function featureRoutes(deps: AppDeps) {
  const { db } = deps.database;
  return new Hono<AppEnv>()
    .get('/', async (c) => c.json(await chatFeatures(db, c.get('user').id)))
    .patch('/', async (c) => c.json(await setChatFeatures(db, c.get('user').id, updateChatFeaturesSchema.parse(await c.req.json()))))
    .get('/default-model', async (c) => c.json(await defaultModel(db, c.get('user').id)))
    .put('/default-model', async (c) => c.json(await setDefaultModel(db, c.get('user').id, defaultModelSchema.parse(await c.req.json()))));
}
