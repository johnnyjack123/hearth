import { type DeviceSelf, pairingDecisionSchema, pairingTokenSchema, startPairingSchema } from '@harness/shared';
import { Hono } from 'hono';
import { type AppDeps, type AppEnv, type DeviceEnv, requireDevice } from '../http/context.ts';
import {
  decidePairing,
  deleteDevice,
  deviceSummary,
  listDevices,
  pairingInfo,
  pollPairing,
  startPairing,
} from '../services/devices.ts';

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
    .get('/', async (c) => c.json(await listDevices(db, c.get('user').id)))
    .delete('/:id', async (c) => {
      await deleteDevice(db, c.get('user').id, c.req.param('id'));
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
    .get('/', (c) => {
      const owner = c.get('owner');
      const body: DeviceSelf = {
        device: deviceSummary(c.get('device')),
        user: { email: owner.email, name: owner.name },
      };
      return c.json(body);
    })
    .delete('/', async (c) => {
      // Logging out on the device revokes its token for good.
      await deleteDevice(db, c.get('owner').id, c.get('device').id);
      return c.body(null, 204);
    });
}
