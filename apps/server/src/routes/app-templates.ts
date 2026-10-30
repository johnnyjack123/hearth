import { BUILTIN_APP_NAMES, createAppTemplateSchema, updateAppTemplateSchema } from '@harness/shared';
import { Hono } from 'hono';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { createAppTemplate, deleteAppTemplate, forDevice, listAppTemplates, updateAppTemplate } from '../services/app-templates.ts';
import { listDevices } from '../services/devices.ts';

/**
 * The signed-in user's own programs (`/api/app-templates`), sent to all of
 * their devices on every change. Mounted behind {@link requireUser}.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/app-templates`.
 */
export function appTemplateRoutes(deps: AppDeps) {
  const { db } = deps.database;

  async function publish(userId: string): Promise<void> {
    deps.hub.sendTemplates(userId, (await listAppTemplates(db, userId)).map(forDevice));
  }

  /**
   * Names the account's devices use for apps of their own. Reports before
   * protocol 6 carry no source; their apps are all the computer's own.
   */
  async function takenNames(userId: string): Promise<Set<string>> {
    const taken = new Set<string>(BUILTIN_APP_NAMES);
    for (const device of await listDevices(db, userId)) {
      const report = deps.hub.status(device.id).apps ?? device.apps;
      for (const app of report?.catalog ?? []) {
        if (app.source !== 'server') {
          taken.add(app.name);
        }
      }
    }
    return taken;
  }

  return new Hono<AppEnv>()
    .get('/', async (c) => c.json(await listAppTemplates(db, c.get('user').id)))
    .post('/', async (c) => {
      const input = createAppTemplateSchema.parse(await c.req.json());
      const userId = c.get('user').id;
      const template = await createAppTemplate(db, userId, input, await takenNames(userId));
      await publish(userId);
      return c.json(template, 201);
    })
    .put('/:id', async (c) => {
      const input = updateAppTemplateSchema.parse(await c.req.json());
      const userId = c.get('user').id;
      const template = await updateAppTemplate(db, userId, c.req.param('id'), input);
      await publish(userId);
      return c.json(template);
    })
    .delete('/:id', async (c) => {
      const userId = c.get('user').id;
      await deleteAppTemplate(db, userId, c.req.param('id'));
      await publish(userId);
      return c.body(null, 204);
    });
}
