import {
  type DeviceSelf,
  type DeviceSummary,
  type DiffAssessment,
  deviceProjectsSchema,
  diffAssessmentRequestSchema,
  pairingDecisionSchema,
  pairingTokenSchema,
  proveDeviceSchema,
  type RemoteProject,
  requestAppFileSchema,
  setProjectAppsSchema,
  startPairingSchema,
  updateDeviceSchema,
} from '@harness/shared';
import { Hono } from 'hono';
import { bodyLimit } from 'hono/body-limit';
import { rateKey } from '../http/client-ip.ts';
import { HttpError, notFound } from '../http/errors.ts';
import { RateLimiter } from '../http/rate-limit.ts';
import { type AppDeps, type AppEnv, type DeviceEnv, requireDevice } from '../http/context.ts';
import {
  decidePairing,
  deleteDevice,
  deviceSummary,
  findDevice,
  listDevices,
  loadDeviceProjects,
  pairingInfo,
  pollPairing,
  proveDevice,
  saveDeviceProjects,
  startPairing,
  updateDevice,
} from '../services/devices.ts';
import { fileStore } from '../files/store.ts';
import { loadHelpers } from '../helpers/load.ts';
import { assessDiff } from '../helpers/review.ts';
import type { DeviceHub } from '../tunnel/hub.ts';

/**
 * A stored device with what its live connection reports.
 *
 * @param hub - Connected devices.
 * @param device - The stored device.
 * @param projects - Its stored projects.
 * @returns The device as the web app shows it.
 */
function withLive(hub: DeviceHub, device: DeviceSummary, projects: RemoteProject[]): DeviceSummary {
  const live = hub.status(device.id);
  // The stored report stands in while the device is offline.
  return { ...device, online: live.online, vm: live.vm, activity: live.activity, projects, apps: live.apps ?? device.apps };
}

/**
 * Unauthenticated pairing endpoints a device calls (`/api/pairing`),
 * modelled on the OAuth device flow (RFC 8628): start, then poll until the
 * user has confirmed the code in the web app.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/pairing`.
 */
export function pairingRoutes(deps: AppDeps) {
  const { db } = deps.database;
  // Anyone may start a pairing; one address must not fill the server's
  // open pairings (50) nor keep its database busy.
  const starts = new RateLimiter(5, 10 * 60_000);
  // A device polls every 2 s; two behind one router stay well below this.
  const polls = new RateLimiter(120, 60_000);
  return new Hono<AppEnv>()
    .use(
      bodyLimit({
        maxSize: 8 * 1024,
        onError: () => {
          throw new HttpError(413, 'validation', 'Request body too large');
        },
      }),
    )
    .post('/', async (c) => {
      starts.check(rateKey(c.get('clientIp')));
      const { name } = startPairingSchema.parse(await c.req.json());
      return c.json(await startPairing(db, deps.config.publicUrl, name, c.get('clientIp')), 201);
    })
    .post('/token', async (c) => {
      polls.check(rateKey(c.get('clientIp')));
      const { deviceCode } = pairingTokenSchema.parse(await c.req.json());
      return c.json(await pollPairing(db, deps.secretBox, deviceCode));
    })
    // `harness login` with a new address of this server keeps its device,
    // once the server proved it holds it (see `proveDevice`).
    .post('/prove', async (c) => {
      polls.check(rateKey(c.get('clientIp')));
      const { lookup, nonce } = proveDeviceSchema.parse(await c.req.json());
      const proof = await proveDevice(db, lookup, nonce);
      if (proof === null) {
        throw notFound('Device');
      }
      return c.json({ proof });
    });
}

/**
 * The signed-in user's devices and pairing decisions (`/api/devices`).
 * Mounted behind {@link requireUser}.
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/devices`.
 */
export function deviceRoutes(deps: AppDeps) {
  const { db } = deps.database;
  return new Hono<AppEnv>()
    .get('/', async (c) => {
      const devices = await listDevices(db, c.get('user').id);
      const projects = await loadDeviceProjects(
        db,
        devices.map((device) => device.id),
      );
      return c.json(devices.map((device) => withLive(deps.hub, device, projects.get(device.id) ?? [])));
    })
    .patch('/:id', async (c) => {
      const input = updateDeviceSchema.parse(await c.req.json());
      const row = await updateDevice(db, c.get('user').id, c.req.param('id'), input);
      if (input.idleMinutes !== undefined) {
        deps.hub.configure(row.id, { idleMinutes: input.idleMinutes });
      }
      const projects = (await loadDeviceProjects(db, [row.id])).get(row.id) ?? [];
      return c.json(withLive(deps.hub, deviceSummary(row), projects));
    })
    .post('/:id/vm/stop', async (c) => {
      const row = await findDevice(db, c.get('user').id, c.req.param('id'));
      if (!row) {
        throw notFound('Device');
      }
      return c.json(await deps.hub.stopVm(row.id));
    })
    .put('/:id/projects/:project/apps', async (c) => {
      const row = await findDevice(db, c.get('user').id, c.req.param('id'));
      if (!row) {
        throw notFound('Device');
      }
      const { enabled } = setProjectAppsSchema.parse(await c.req.json());
      const project = c.req.param('project');
      // The device checks again; this only spares it requests that cannot work.
      if (!deps.hub.status(row.id).projects.some((offered) => offered.name === project)) {
        throw new HttpError(409, 'conflict', 'The device does not offer this project');
      }
      return c.json(await deps.hub.setApps(row.id, project, enabled));
    })
    .post('/:id/app-files', async (c) => {
      const row = await findDevice(db, c.get('user').id, c.req.param('id'));
      if (!row) {
        throw notFound('Device');
      }
      const file = requestAppFileSchema.parse(await c.req.json());
      return c.json(await deps.hub.requestAppFile(row.id, file));
    })
    .delete('/:id', async (c) => {
      await deleteDevice(db, c.get('user').id, c.req.param('id'));
      deps.hub.disconnect(c.req.param('id'));
      return c.body(null, 204);
    })
    .get('/pairing/:userCode', async (c) => c.json(await pairingInfo(db, c.req.param('userCode'), c.get('clientIp'))))
    .post('/pairing/:userCode', async (c) => {
      const { approve } = pairingDecisionSchema.parse(await c.req.json());
      await decidePairing(db, deps.secretBox, c.get('user').id, c.req.param('userCode'), approve);
      return c.json(await pairingInfo(db, c.req.param('userCode'), c.get('clientIp')));
    });
}

/**
 * Endpoints a paired device calls about itself (`/api/device`).
 *
 * @param deps - Application dependencies.
 * @returns Router to mount under `/api/device`.
 */
export function deviceSelfRoutes(deps: AppDeps) {
  const { db } = deps.database;
  const files = fileStore(db, deps.config.dataDir);

  /**
   * A file the device may read: the owner's, attached to a chat that is
   * linked to this device. A device of the same account cannot read the
   * files of chats on another computer.
   */
  async function deviceFile(owner: string, deviceId: string, id: string) {
    const file = await files.find(owner, id);
    const linked =
      file?.conversationId &&
      (await db
        .selectFrom('chatLink')
        .select('conversationId')
        .where('conversationId', '=', file.conversationId)
        .where('deviceId', '=', deviceId)
        .executeTakeFirst());
    if (!file || !linked) {
      throw notFound('File');
    }
    return file;
  }

  return new Hono<DeviceEnv>()
    .use(requireDevice(db))
    // The original bytes of a file attached in a linked chat.
    .get('/files/:id', async (c) => {
      const file = await deviceFile(c.get('owner').id, c.get('device').id, c.req.param('id'));
      return c.body(new Uint8Array(await files.read(file.id)), 200, { 'content-type': 'application/octet-stream' });
    })
    // The text the model would read (documents as Markdown).
    .get('/files/:id/text', async (c) => {
      const file = await deviceFile(c.get('owner').id, c.get('device').id, c.req.param('id'));
      if (file.text === null) {
        throw notFound('File text');
      }
      return c.body(file.text, 200, { 'content-type': 'text/plain; charset=utf-8' });
    })
    .get('/', async (c) => {
      const owner = c.get('owner');
      const device = c.get('device');
      const projects = (await loadDeviceProjects(db, [device.id])).get(device.id) ?? [];
      const body: DeviceSelf = {
        device: withLive(deps.hub, deviceSummary(device), projects),
        user: { email: owner.email, name: owner.name },
      };
      return c.json(body);
    })
    .post('/assessments/diff', async (c) => {
      // Jev for the merge check on the computer, billed to the user's own
      // OpenRouter key like the helpers of their chats.
      const { files } = diffAssessmentRequestSchema.parse(await c.req.json());
      // Switched off for this device in the web app (Settings → Devices).
      if (c.get('device').jevReview === 0) {
        const off: DiffAssessment = { model: null, files: null, cost: null, reason: 'disabled' };
        return c.json(off);
      }
      const helpers = await loadHelpers(db, deps.secretBox, c.get('owner').id, null, deps.helpers);
      if (!helpers) {
        const none: DiffAssessment = { model: null, files: null, cost: null, reason: 'no_helpers' };
        return c.json(none);
      }
      const dangers = await assessDiff(helpers, files);
      const costs = helpers.usage.map((call) => call.cost?.total);
      const body: DiffAssessment = {
        model: helpers.jevModel,
        files: dangers,
        cost: costs.length > 0 && costs.every((cost) => cost !== undefined) ? costs.reduce((sum, cost) => sum + (cost ?? 0), 0) : null,
        ...(dangers ? {} : { reason: 'failed' as const }),
      };
      return c.json(body);
    })
    .put('/projects', async (c) => {
      // `harness init` reports a new project right away, also when the
      // background service is not connected.
      const { projects } = deviceProjectsSchema.parse(await c.req.json());
      await saveDeviceProjects(db, c.get('device').id, projects);
      return c.body(null, 204);
    })
    .delete('/', async (c) => {
      // Logging out on the device revokes its token for good.
      await deleteDevice(db, c.get('owner').id, c.get('device').id);
      deps.hub.disconnect(c.get('device').id);
      return c.body(null, 204);
    });
}
