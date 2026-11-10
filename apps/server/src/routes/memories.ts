import { memoryInputSchema } from '@hearth/shared';
import { Hono } from 'hono';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { notFound } from '../http/errors.ts';
import { addMemory, confirmMemory, listMemories, removeMemory, updateMemory } from '../services/memories.ts';

/**
 * The signed-in user's memories (`/api/memories`): what every chat and agent
 * is told. The user sees, changes and removes them here, also those a model
 * stored. Mounted behind {@link requireUser}.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/memories`.
 */
export function memoryRoutes(deps: AppDeps) {
  const { db } = deps.database;
  return new Hono<AppEnv>()
    .get('/', async (c) => c.json(await listMemories(db, c.get('user').id)))
    .post('/', async (c) => {
      const { text } = memoryInputSchema.parse(await c.req.json());
      return c.json(await addMemory(db, c.get('user').id, { text, source: 'user' }), 201);
    })
    .put('/:id', async (c) => {
      const { text } = memoryInputSchema.parse(await c.req.json());
      return c.json(await updateMemory(db, c.get('user').id, c.req.param('id'), text));
    })
    // A model's suggestion becomes a memory every chat reads.
    .post('/:id/confirm', async (c) => c.json(await confirmMemory(db, c.get('user').id, c.req.param('id'))))
    .delete('/:id', async (c) => {
      if (!(await removeMemory(db, c.get('user').id, c.req.param('id')))) {
        throw notFound('Memory');
      }
      return c.body(null, 204);
    });
}
