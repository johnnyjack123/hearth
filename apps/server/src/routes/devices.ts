import {
  type DeviceSelf,
  type DeviceSummary,
  type DiffAssessment,
  deviceProjectsSchema,
  diffAssessmentRequestSchema,
  pairingDecisionSchema,
  pairingTokenSchema,
  type RemoteProject,
  startPairingSchema,
  updateDeviceSchema,
} from '@harness/shared';
import { Hono } from 'hono';
import { notFound } from '../http/errors.ts';
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
  saveDeviceProjects,
  startPairing,
  updateDevice,
} from '../services/devices.ts';
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
  return { ...device, online: live.online, vm: live.vm, activity: live.activity, projects };
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
  return new Hono()
    .post('/', async (c) => {
      const { name } = startPairingSchema.parse(await c.req.json());
      return c.json(await startPairing(db, deps.config.publicUrl, name), 201);
    })
    .post('/token', async (c) => {
      const { deviceCode } = pairingTokenSchema.parse(await c.req.json());
      return c.json(await pollPairing(db, deps.secretBox, deviceCode));
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
    .delete('/:id', async (c) => {
      await deleteDevice(db, c.get('user').id, c.req.param('id'));
      deps.hub.disconnect(c.req.param('id'));
      return c.body(null, 204);
    })
    .get('/pairing/:userCode', async (c) => c.json(await pairingInfo(db, c.req.param('userCode'))))
    .post('/pairing/:userCode', async (c) => {
      const { approve } = pairingDecisionSchema.parse(await c.req.json());
      await decidePairing(db, deps.secretBox, c.get('user').id, c.req.param('userCode'), approve);
      return c.json(await pairingInfo(db, c.req.param('userCode')));
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
  return new Hono<DeviceEnv>()
    .use(requireDevice(db))
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
