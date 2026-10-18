import { createHash, randomBytes, randomInt } from 'node:crypto';
import type { DeviceSummary, PairingInfo, PairingPoll, PairingStart, RemoteProject } from '@harness/shared';
import type { Kysely, Selectable } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import type { SecretBox } from '../crypto.ts';
import type { DB, DeviceTable, UserTable } from '../db/schema.ts';
import { HttpError, notFound } from '../http/errors.ts';

/** A device row as read from the database. */
export type DeviceRow = Selectable<DeviceTable>;

/** The account a device acts for. */
export type DeviceOwner = Pick<Selectable<UserTable>, 'id' | 'email' | 'name' | 'role'>;

/** How long a pairing waits for the user's confirmation. */
export const PAIRING_TTL_MS = 10 * 60_000;

/** Seconds a device waits between polls; the server is usually local. */
export const POLL_INTERVAL_S = 2;

/** Open pairings at once; pairing starts are unauthenticated. */
const MAX_PENDING = 50;

/** `lastUsedAt` is written at most this often per device. */
const TOUCH_INTERVAL_MS = 60_000;

// Without 0/O and 1/I, so a code read off a screen cannot be mistyped.
const USER_CODE_ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
const USER_CODE_LENGTH = 8;

/**
 * Hashes a token or device code for storage. Both are 256-bit random values,
 * so a fast hash suffices; a database leak does not reveal usable tokens.
 *
 * @param secret - Token or device code.
 * @returns Hex-encoded SHA-256.
 */
export function hashSecret(secret: string): string {
  return createHash('sha256').update(secret, 'utf8').digest('hex');
}

function randomToken(prefix: string): string {
  return `${prefix}${randomBytes(32).toString('base64url')}`;
}

function newUserCode(): string {
  let code = '';
  for (let i = 0; i < USER_CODE_LENGTH; i += 1) {
    code += USER_CODE_ALPHABET[randomInt(USER_CODE_ALPHABET.length)];
  }
  return code;
}

/**
 * Formats a stored user code for display.
 *
 * @param code - Code without separator.
 * @returns The code as `ABCD-EFGH`.
 */
export function formatUserCode(code: string): string {
  return `${code.slice(0, 4)}-${code.slice(4)}`;
}

/**
 * Normalizes a user code as typed by a person: case, dash and spaces do not
 * matter.
 *
 * @param input - Code as entered.
 * @returns The stored form, or null if it cannot be a valid code.
 */
export function normalizeUserCode(input: string): string | null {
  const code = input.toUpperCase().replace(/[\s-]/g, '');
  const valid = code.length === USER_CODE_LENGTH && [...code].every((c) => USER_CODE_ALPHABET.includes(c));
  return valid ? code : null;
}

// Online state lives in the tunnel hub; routes that know it fill it in.
function toSummary(row: DeviceRow): DeviceSummary {
  return {
    id: row.id,
    name: row.name,
    createdAt: row.createdAt,
    lastUsedAt: row.lastUsedAt,
    online: false,
    projects: [],
    vm: null,
  };
}

/**
 * Replaces the projects a device offers.
 *
 * @param db - Open database.
 * @param deviceId - The device.
 * @param projects - Its projects now.
 */
export async function saveDeviceProjects(db: Kysely<DB>, deviceId: string, projects: RemoteProject[]): Promise<void> {
  const now = Date.now();
  await db.transaction().execute(async (trx) => {
    await trx.deleteFrom('deviceProject').where('deviceId', '=', deviceId).execute();
    if (projects.length > 0) {
      await trx
        .insertInto('deviceProject')
        .values(projects.map((project) => ({ deviceId, name: project.name, harnesses: JSON.stringify(project.harnesses), updatedAt: now })))
        .execute();
    }
  });
}

/**
 * The projects devices offer, by device id.
 *
 * @param db - Open database.
 * @param deviceIds - Devices to look up.
 * @returns Projects per device; devices without any are missing.
 */
export async function loadDeviceProjects(db: Kysely<DB>, deviceIds: string[]): Promise<Map<string, RemoteProject[]>> {
  const result = new Map<string, RemoteProject[]>();
  if (deviceIds.length === 0) {
    return result;
  }
  const rows = await db
    .selectFrom('deviceProject')
    .selectAll()
    .where('deviceId', 'in', deviceIds)
    .orderBy('name', 'asc')
    .execute();
  for (const row of rows) {
    const list = result.get(row.deviceId) ?? [];
    list.push({ name: row.name, harnesses: JSON.parse(row.harnesses) as string[] });
    result.set(row.deviceId, list);
  }
  return result;
}

/**
 * Starts a pairing for a device that wants to act for an account.
 *
 * @param db - Open database.
 * @param publicUrl - Origin of the web app, for the confirmation link.
 * @param name - Name the device reports.
 * @returns Codes and timing for the device.
 * @throws {HttpError} 429 if too many pairings are open.
 */
export async function startPairing(db: Kysely<DB>, publicUrl: string, name: string): Promise<PairingStart> {
  const now = Date.now();
  // Finished or expired pairings are only kept briefly for the final poll.
  await db.deleteFrom('devicePairing').where('expiresAt', '<', now - PAIRING_TTL_MS).execute();
  const open = await db
    .selectFrom('devicePairing')
    .select((eb) => eb.fn.countAll<number>().as('count'))
    .where('status', '=', 'pending')
    .where('expiresAt', '>', now)
    .executeTakeFirstOrThrow();
  if (Number(open.count) >= MAX_PENDING) {
    throw new HttpError(429, 'too_many_requests', 'Too many pairings in progress');
  }

  const deviceCode = randomToken('');
  const userCode = newUserCode();
  await db
    .insertInto('devicePairing')
    .values({
      id: uuidv7(),
      deviceCodeHash: hashSecret(deviceCode),
      userCode,
      name,
      status: 'pending',
      userId: null,
      deviceId: null,
      encryptedToken: null,
      createdAt: now,
      expiresAt: now + PAIRING_TTL_MS,
    })
    .execute();
  const verificationUri = `${publicUrl}/device`;
  return {
    deviceCode,
    userCode: formatUserCode(userCode),
    verificationUri,
    verificationUriComplete: `${verificationUri}?code=${formatUserCode(userCode)}`,
    expiresIn: PAIRING_TTL_MS / 1000,
    interval: POLL_INTERVAL_S,
  };
}

/**
 * Answers a device's poll. The token of an approved pairing is handed out
 * once and then deleted from the pairing.
 *
 * @param db - Open database.
 * @param box - Encryption that holds the token until pickup.
 * @param deviceCode - The device's secret.
 * @returns The pairing state, with the token on success.
 */
export async function pollPairing(db: Kysely<DB>, box: SecretBox, deviceCode: string): Promise<PairingPoll> {
  const row = await db
    .selectFrom('devicePairing')
    .selectAll()
    .where('deviceCodeHash', '=', hashSecret(deviceCode))
    .executeTakeFirst();
  // Unknown codes look like expired ones, so guessing reveals nothing.
  if (!row) {
    return { status: 'expired' };
  }
  if (row.status === 'denied') {
    return { status: 'denied' };
  }
  if (row.status === 'pending') {
    return row.expiresAt > Date.now() ? { status: 'pending' } : { status: 'expired' };
  }
  if (!row.encryptedToken || !row.deviceId) {
    return { status: 'expired' };
  }
  // Clearing the token is the claim: of two concurrent polls only one wins.
  const claimed = await db
    .updateTable('devicePairing')
    .set({ encryptedToken: null })
    .where('id', '=', row.id)
    .where('encryptedToken', '=', row.encryptedToken)
    .executeTakeFirst();
  if (Number(claimed.numUpdatedRows) !== 1) {
    return { status: 'expired' };
  }
  const token = box.decrypt(row.encryptedToken, `pairing:${row.id}`);
  return { status: 'approved', deviceId: row.deviceId, token };
}

async function findPairing(db: Kysely<DB>, userCode: string) {
  const code = normalizeUserCode(userCode);
  const row = code
    ? await db.selectFrom('devicePairing').selectAll().where('userCode', '=', code).executeTakeFirst()
    : undefined;
  if (!row) {
    throw notFound('Pairing');
  }
  return row;
}

/**
 * Describes a pairing for the confirmation page.
 *
 * @param db - Open database.
 * @param userCode - Code as entered by the user.
 * @returns The pairing.
 * @throws {HttpError} 404 if there is no such pairing.
 */
export async function pairingInfo(db: Kysely<DB>, userCode: string): Promise<PairingInfo> {
  const row = await findPairing(db, userCode);
  const expired = row.status === 'pending' && row.expiresAt <= Date.now();
  return {
    userCode: formatUserCode(row.userCode),
    name: row.name,
    status: expired ? 'expired' : row.status,
    expiresAt: row.expiresAt,
  };
}

/**
 * Approves or denies a pairing. Approval creates the device and its token.
 *
 * @param db - Open database.
 * @param box - Encryption that holds the token until the device picks it up.
 * @param userId - Account that decides and, on approval, owns the device.
 * @param userCode - Code as entered by the user.
 * @param approve - Whether to approve.
 * @throws {HttpError} 404 if there is no such pairing, 400 if it is no longer pending.
 */
export async function decidePairing(
  db: Kysely<DB>,
  box: SecretBox,
  userId: string,
  userCode: string,
  approve: boolean,
): Promise<void> {
  const row = await findPairing(db, userCode);
  if (row.status !== 'pending' || row.expiresAt <= Date.now()) {
    throw new HttpError(400, 'validation', 'Pairing is no longer pending');
  }
  if (!approve) {
    await db
      .updateTable('devicePairing')
      .set({ status: 'denied', userId })
      .where('id', '=', row.id)
      .where('status', '=', 'pending')
      .execute();
    return;
  }
  const token = randomToken('hd_');
  const deviceId = uuidv7();
  await db.transaction().execute(async (trx) => {
    const updated = await trx
      .updateTable('devicePairing')
      .set({ status: 'approved', userId, deviceId, encryptedToken: box.encrypt(token, `pairing:${row.id}`) })
      .where('id', '=', row.id)
      .where('status', '=', 'pending')
      .executeTakeFirst();
    if (Number(updated.numUpdatedRows) !== 1) {
      throw new HttpError(400, 'validation', 'Pairing is no longer pending');
    }
    await trx
      .insertInto('device')
      .values({
        id: deviceId,
        userId,
        name: row.name,
        tokenHash: hashSecret(token),
        createdAt: Date.now(),
        lastUsedAt: null,
      })
      .execute();
  });
}

/**
 * Lists a user's devices, newest first.
 *
 * @param db - Open database.
 * @param userId - Owner.
 * @returns The devices.
 */
export async function listDevices(db: Kysely<DB>, userId: string): Promise<DeviceSummary[]> {
  const rows = await db
    .selectFrom('device')
    .selectAll()
    .where('userId', '=', userId)
    .orderBy('createdAt', 'desc')
    .execute();
  return rows.map(toSummary);
}

/**
 * Finds one of a user's devices.
 *
 * @param db - Open database.
 * @param userId - Expected owner.
 * @param id - Device id.
 * @returns The device, or undefined if missing or owned by someone else.
 */
export async function findDevice(db: Kysely<DB>, userId: string, id: string): Promise<DeviceRow | undefined> {
  return db.selectFrom('device').selectAll().where('id', '=', id).where('userId', '=', userId).executeTakeFirst();
}

/**
 * Removes a device; its token stops working immediately.
 *
 * @param db - Open database.
 * @param userId - Expected owner.
 * @param id - Device id.
 * @throws {HttpError} 404 if the device does not exist or belongs to someone else.
 */
export async function deleteDevice(db: Kysely<DB>, userId: string, id: string): Promise<void> {
  const result = await db.deleteFrom('device').where('id', '=', id).where('userId', '=', userId).executeTakeFirst();
  if (Number(result.numDeletedRows) !== 1) {
    throw notFound('Device');
  }
}

/**
 * Resolves a device token and records the use.
 *
 * @param db - Open database.
 * @param token - Token from the `Authorization` header.
 * @returns The device and its owner, or undefined for an unknown token.
 */
export async function authenticateDevice(
  db: Kysely<DB>,
  token: string,
): Promise<{ device: DeviceRow; owner: DeviceOwner } | undefined> {
  const found = await db
    .selectFrom('device')
    .innerJoin('user', 'user.id', 'device.userId')
    .selectAll('device')
    .select(['user.email as ownerEmail', 'user.name as ownerName', 'user.role as ownerRole'])
    .where('device.tokenHash', '=', hashSecret(token))
    .executeTakeFirst();
  if (!found) {
    return undefined;
  }
  const { ownerEmail, ownerName, ownerRole, ...device } = found;
  const now = Date.now();
  if (device.lastUsedAt === null || now - device.lastUsedAt > TOUCH_INTERVAL_MS) {
    await db.updateTable('device').set({ lastUsedAt: now }).where('id', '=', device.id).execute();
    device.lastUsedAt = now;
  }
  return {
    device,
    owner: { id: device.userId, email: ownerEmail, name: ownerName, role: ownerRole },
  };
}

/**
 * The public view of a device.
 *
 * @param row - Device row.
 * @returns The summary.
 */
export function deviceSummary(row: DeviceRow): DeviceSummary {
  return toSummary(row);
}
