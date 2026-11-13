import { removeTagSchema, renameTagSchema } from '@hearth/shared';
import { Hono } from 'hono';
import { clientSource, LiveEvents } from '../events.ts';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { notFound } from '../http/errors.ts';
import { removeTag, renameTag } from '../services/conversations.ts';

/**
 * The signed-in user's tags across all chats: rename one, or take it off
 * every chat. Which tags a chat has is set on the chat itself.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/tags`.
 */
export function tagRoutes(deps: AppDeps) {
  const { db } = deps.database;
  const events = deps.events ?? new LiveEvents();
  return new Hono<AppEnv>()
    .patch('/', async (c) => {
      const { from, to } = renameTagSchema.parse(await c.req.json());
      const chats = await renameTag(db, c.get('user').id, from, to);
      if (chats === 0) {
        throw notFound('Tag');
      }
      events.list(c.get('user').id, clientSource(c));
      return c.json({ chats });
    })
    .delete('/', async (c) => {
      const { name } = removeTagSchema.parse(await c.req.json());
      const chats = await removeTag(db, c.get('user').id, name);
      if (chats === 0) {
        throw notFound('Tag');
      }
      events.list(c.get('user').id, clientSource(c));
      return c.json({ chats });
    });
}
