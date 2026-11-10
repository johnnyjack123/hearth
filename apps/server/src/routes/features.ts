import { updateChatFeaturesSchema } from '@harness/shared';
import { Hono } from 'hono';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { chatFeatures, setChatFeatures } from '../services/features.ts';

/**
 * The signed-in user's chat features (`/api/features`): which tools their
 * chats and agents get, and how OpenRouter picks hosts for them.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/features`.
 */
export function featureRoutes(deps: AppDeps) {
  const { db } = deps.database;
  return new Hono<AppEnv>()
    .get('/', async (c) => c.json(await chatFeatures(db, c.get('user').id)))
    .patch('/', async (c) => c.json(await setChatFeatures(db, c.get('user').id, updateChatFeaturesSchema.parse(await c.req.json()))));
}
