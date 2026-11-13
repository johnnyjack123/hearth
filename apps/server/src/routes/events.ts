import type { LiveEvent } from '@hearth/shared';
import { Hono } from 'hono';
import { streamSSE } from 'hono/streaming';
import { LiveEvents } from '../events.ts';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { SERVER_COMMIT, SERVER_VERSION } from '../version.ts';

/**
 * A ping this often: proxies keep the connection open, and a page that
 * hears nothing for longer knows it died (phones drop connections of
 * pages in the background without saying so).
 */
const PING_MS = 20_000;

/**
 * `GET /api/events`: what changes in the user's chats, as server-sent
 * events (see {@link LiveEvent}).
 *
 * @param deps - Application dependencies.
 * @returns The router.
 */
export function eventRoutes(deps: AppDeps) {
  const events = deps.events ?? new LiveEvents();
  return new Hono<AppEnv>().get('/', (c) => {
    const userId = c.get('user').id;
    return streamSSE(c, async (stream) => {
      const queue: LiveEvent[] = [];
      let wake = () => {};
      const unsubscribe = events.subscribe(userId, (event) => {
        queue.push(event);
        wake();
      });
      stream.onAbort(() => {
        unsubscribe();
        wake();
      });
      try {
        const hello: LiveEvent = { type: 'hello', version: SERVER_VERSION, commit: SERVER_COMMIT };
        await stream.writeSSE({ event: hello.type, data: JSON.stringify(hello) });
        while (!stream.aborted) {
          if (queue.length === 0) {
            await new Promise<void>((resolve) => {
              const timer = setTimeout(resolve, PING_MS);
              wake = () => {
                clearTimeout(timer);
                resolve();
              };
            });
            wake = () => {};
          }
          if (stream.aborted) {
            break;
          }
          const batch: LiveEvent[] = queue.length > 0 ? queue.splice(0) : [{ type: 'ping' }];
          for (const event of batch) {
            await stream.writeSSE({ event: event.type, data: JSON.stringify(event) });
          }
        }
      } finally {
        unsubscribe();
      }
    });
  });
}
