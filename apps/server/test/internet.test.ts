import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import type { PairingInfo, PairingStart } from '@hearth/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { loadConfig } from '../src/config.ts';
import { parseTrustedProxies, rateKey, resolveClientIp } from '../src/http/client-ip.ts';
import { RateLimiter } from '../src/http/rate-limit.ts';
import { SETUP_CODE_HEADER, sameSetupCode } from '../src/routes/auth.ts';
import { startupNotes } from '../src/server.ts';
import { isBanned } from '../src/services/devices.ts';
import { createTestServer, DIALECTS, ORIGIN, SETUP_CODE, type TestServer } from './helpers.ts';

const CLI = { origin: '' };

describe('client address', () => {
  const proxies = parseTrustedProxies('172.16.0.0/12, 127.0.0.1, fd00::/8');

  it('takes the connection when it is no proxy, whatever the header says', () => {
    expect(resolveClientIp('203.0.113.7', '1.2.3.4', proxies)).toBe('203.0.113.7');
    expect(resolveClientIp('::ffff:203.0.113.7', undefined, proxies)).toBe('203.0.113.7');
    expect(resolveClientIp(null, '1.2.3.4', proxies)).toBeNull();
  });

  it('reads X-Forwarded-For from the right behind a trusted proxy', () => {
    expect(resolveClientIp('172.18.0.5', '198.51.100.9', proxies)).toBe('198.51.100.9');
    // A client's own header comes first; the proxy appends the real address.
    expect(resolveClientIp('172.18.0.5', '6.6.6.6, 198.51.100.9', proxies)).toBe('198.51.100.9');
    // Proxies in a row are skipped.
    expect(resolveClientIp('127.0.0.1', '198.51.100.9, 172.18.0.2', proxies)).toBe('198.51.100.9');
    expect(resolveClientIp('fd00::1', '2001:db8::5', proxies)).toBe('2001:db8::5');
    // Garbage or only proxies: the proxy itself.
    expect(resolveClientIp('172.18.0.5', 'nonsense', proxies)).toBe('172.18.0.5');
    expect(resolveClientIp('172.18.0.5', '127.0.0.1', proxies)).toBe('172.18.0.5');
    expect(resolveClientIp('172.18.0.5', undefined, proxies)).toBe('172.18.0.5');
  });

  it('rejects proxy entries it cannot read', () => {
    expect(parseTrustedProxies(undefined).any).toBe(false);
    expect(parseTrustedProxies('').has('127.0.0.1')).toBe(false);
    for (const bad of ['localhost', '10.0.0.0/33', '10.0.0.0/', '10.0.0.0/8/1', 'fd00::/129']) {
      expect(() => parseTrustedProxies(bad)).toThrow(/TRUSTED_PROXIES/);
    }
  });

  it('counts IPv6 clients by their /64', () => {
    expect(rateKey('2001:db8:1:2:aaaa::1')).toBe('2001:db8:1:2::/64');
    expect(rateKey('2001:db8:1:2:bbbb:cccc:dddd:eeee')).toBe('2001:db8:1:2::/64');
    expect(rateKey('2001:db8::1')).toBe('2001:db8:0:0::/64');
    expect(rateKey('198.51.100.9')).toBe('198.51.100.9');
    expect(rateKey(null)).toBe('unknown');
  });
});

describe('small pieces', () => {
  it('limits per key and window', () => {
    let now = 0;
    const limiter = new RateLimiter(2, 1000, () => now);
    expect(limiter.take('a')).toBe(0);
    expect(limiter.take('a')).toBe(0);
    expect(limiter.take('a')).toBe(1);
    expect(limiter.take('b')).toBe(0);
    now = 1000;
    expect(limiter.take('a')).toBe(0);
  });

  it('keeps a ban until its end', () => {
    expect(isBanned(null, null)).toBe(false);
    expect(isBanned(0, null)).toBe(false);
    expect(isBanned(1, null)).toBe(true);
    expect(isBanned(true, new Date(2_000), 1_000)).toBe(true);
    expect(isBanned(true, 500, 1_000)).toBe(false);
    expect(isBanned(1, new Date(500).toISOString(), 1_000)).toBe(false);
    expect(isBanned(1, 'not a date', 1_000)).toBe(true);
  });

  it('compares setup codes like a person types them', () => {
    expect(sameSetupCode('abcd efgh', 'ABCD-EFGH')).toBe(true);
    expect(sameSetupCode('ABCD-EFGX', 'ABCD-EFGH')).toBe(false);
    expect(sameSetupCode(undefined, 'ABCD-EFGH')).toBe(false);
  });

  it('logs the setup code and warns about a proxy without TRUSTED_PROXIES', () => {
    const base = { publicUrl: 'https://chat.example.org', setupCode: 'ABCD-EFGH' } as Parameters<typeof startupNotes>[0];
    const fresh = startupNotes(base, 0);
    expect(fresh.join('\n')).toMatch(/ABCD-EFGH/);
    expect(fresh.join('\n')).toMatch(/TRUSTED_PROXIES/);
    expect(startupNotes({ ...base, trustedProxies: parseTrustedProxies('172.16.0.0/12') }, 3)).toEqual([]);
  });

  it('refuses a stored secret that is too short', () => {
    const dataDir = mkdtempSync(path.join(tmpdir(), 'hearth-secret-'));
    writeFileSync(path.join(dataDir, 'server-secret'), 'short\n');
    expect(() => loadConfig({ DATA_DIR: dataDir })).toThrow(/at least 32/);
  });

  it('makes a setup code unless one is given', () => {
    const dataDir = mkdtempSync(path.join(tmpdir(), 'hearth-setup-'));
    expect(loadConfig({ DATA_DIR: dataDir }).setupCode).toMatch(/^[A-Z2-9]{4}-[A-Z2-9]{4}$/);
    expect(loadConfig({ DATA_DIR: dataDir, HEARTH_SETUP_CODE: 'my-own-code' }).setupCode).toBe('my-own-code');
    expect(() => loadConfig({ DATA_DIR: dataDir, TRUSTED_PROXIES: 'proxy' })).toThrow(/TRUSTED_PROXIES/);
  });
});

describe.each(DIALECTS)('server on the internet (%s)', (kind) => {
  let server: TestServer;

  beforeEach(async () => {
    server = await createTestServer(kind);
  });

  afterEach(async () => {
    await server.close();
  });

  const signUp = (email: string, headers: Record<string, string> = {}) =>
    server.request('POST', '/api/auth/sign-up/email', { body: { email, name: 'x', password: 'correct-horse-battery' }, headers });

  it('lets only the holder of the setup code claim a fresh server', async () => {
    const without = await signUp('intruder@example.com');
    expect(without.status).toBe(403);
    expect(await without.json()).toMatchObject({ code: 'SETUP_CODE_INVALID' });
    expect((await signUp('intruder@example.com', { [SETUP_CODE_HEADER]: 'WRONG-CODE' })).status).toBe(403);
    // Case and dash do not matter.
    expect((await signUp('owner@example.com', { [SETUP_CODE_HEADER]: SETUP_CODE.toLowerCase().replace('-', '') })).status).toBe(200);
    // Later accounts follow the registration setting, without a code.
    expect((await signUp('second@example.com')).status).toBe(200);
  });

  it('turns sign-ups away early when registration is closed, so it reveals no accounts', async () => {
    const admin = await server.signUp('admin@example.com');
    await server.signUp('member@example.com');
    await server.request('PATCH', '/api/admin/settings', { cookie: admin, body: { registration: 'closed' } });
    for (const email of ['member@example.com', 'new@example.com']) {
      const response = await signUp(email);
      expect(response.status).toBe(403);
      expect(await response.json()).toMatchObject({ code: 'REGISTRATION_CLOSED' });
    }
  });

  it('rejects oversized bodies before Better Auth reads them', async () => {
    const response = await server.request('POST', '/api/auth/sign-in/email', {
      body: { email: 'a@example.com', password: 'x'.repeat(70 * 1024) },
    });
    expect(response.status).toBe(413);
    const pairing = await server.request('POST', '/api/pairing', { body: { name: 'x'.repeat(10 * 1024) }, headers: CLI });
    expect(pairing.status).toBe(413);
  });

  it('shows where a pairing came from and whether that is the browser’s address', async () => {
    const cookie = await server.signUp('owner@example.com');
    // Through the proxy (10.0.0.0/8 in tests): its header names the client.
    const started = await server.request('POST', '/api/pairing', {
      body: { name: 'hearth auf PC' },
      headers: { ...CLI, 'x-forwarded-for': '198.51.100.9' },
      peer: '10.0.0.2',
    });
    const { userCode } = (await started.json()) as PairingStart;
    const same = (await (await server.request('GET', `/api/devices/pairing/${userCode}`, { cookie, peer: '198.51.100.9' })).json()) as PairingInfo;
    expect(same).toMatchObject({ ip: '198.51.100.9', sameAddress: true });
    expect(same.createdAt).toBeGreaterThan(0);
    const other = (await (await server.request('GET', `/api/devices/pairing/${userCode}`, { cookie, peer: '203.0.113.50' })).json()) as PairingInfo;
    expect(other).toMatchObject({ ip: '198.51.100.9', sameAddress: false });

    // Straight to the server, the header counts for nothing.
    const direct = await server.request('POST', '/api/pairing', {
      body: { name: 'hearth auf PC' },
      headers: { ...CLI, 'x-forwarded-for': '198.51.100.9' },
      peer: '203.0.113.66',
    });
    const spoofed = (await direct.json()) as PairingStart;
    const info = (await (await server.request('GET', `/api/devices/pairing/${spoofed.userCode}`, { cookie })).json()) as PairingInfo;
    expect(info).toMatchObject({ ip: '203.0.113.66', sameAddress: null });
  });

  it('limits pairing starts per address', async () => {
    const start = (peer: string) => server.request('POST', '/api/pairing', { body: { name: 'PC' }, headers: CLI, peer });
    for (let index = 0; index < 5; index += 1) {
      expect((await start('203.0.113.1')).status).toBe(201);
    }
    expect((await start('203.0.113.1')).status).toBe(429);
    expect((await start('203.0.113.2')).status).toBe(201);
  });

  it('stops the devices of a banned account until the ban is lifted', async () => {
    const admin = await server.signUp('admin@example.com');
    const member = await server.signUp('member@example.com');
    const started = (await (await server.request('POST', '/api/pairing', { body: { name: 'PC' }, headers: CLI })).json()) as PairingStart;
    await server.request('POST', `/api/devices/pairing/${started.userCode}`, { cookie: member, body: { approve: true } });
    const { token } = (await (await server.request('POST', '/api/pairing/token', { body: { deviceCode: started.deviceCode }, headers: CLI })).json()) as {
      token: string;
    };
    const asDevice = () => server.request('GET', '/api/device', { headers: { ...CLI, authorization: `Bearer ${token}` } });
    expect((await asDevice()).status).toBe(200);

    const user = await server.database.db.selectFrom('user').select('id').where('email', '=', 'member@example.com').executeTakeFirstOrThrow();
    expect((await server.request('POST', '/api/auth/admin/ban-user', { cookie: admin, body: { userId: user.id } })).status).toBe(200);
    expect((await asDevice()).status).toBe(401);
    expect((await server.request('POST', '/api/auth/admin/unban-user', { cookie: admin, body: { userId: user.id } })).status).toBe(200);
    expect((await asDevice()).status).toBe(200);
  });

  it('proves it holds a device without the token travelling, as hearth computes it', async () => {
    const cookie = await server.signUp('owner@example.com');
    const started = (await (await server.request('POST', '/api/pairing', { body: { name: 'PC' }, headers: CLI })).json()) as PairingStart;
    await server.request('POST', `/api/devices/pairing/${started.userCode}`, { cookie, body: { approve: true } });
    // The token of the fixed values in `hearth-core/src/server.rs`.
    await server.database.db.updateTable('device').set({ tokenHash: 'a71e718f66da4e3bde5b7756f407734208d50d07f5e67f411329e17883285433' }).execute();
    const prove = (lookup: string) =>
      server.request('POST', '/api/pairing/prove', { body: { lookup, nonce: '00112233445566778899aabbccddeeff' }, headers: CLI });
    const known = await prove('a0cc9c1632b9567bfe8d4d7aa836e1298dbf103fcfdcada8481e691668153c1d');
    expect(await known.json()).toEqual({ proof: 'dd44dcd282ec2ec506d61ae11f379095c42c7b06e3cd4eef2dd58c5b28586449' });
    expect((await prove('0'.repeat(64))).status).toBe(404);
    expect((await server.request('POST', '/api/pairing/prove', { body: { lookup: 'x', nonce: 'y' }, headers: CLI })).status).toBe(400);
  });

  it('serves uploaded files under their own sandboxing policy', async () => {
    const cookie = await server.signUp('owner@example.com');
    const uploaded = await server.app.request('/api/files', {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/octet-stream', 'x-file-name': 'notiz.txt' },
      body: new TextEncoder().encode('<script>alert(1)</script>'),
    });
    expect(uploaded.status).toBe(201);
    const { id } = (await uploaded.json()) as { id: string };
    const file = await server.request('GET', `/api/files/${id}`, { cookie });
    expect(file.headers.get('content-security-policy')).toBe("default-src 'none'; sandbox");
    // The app keeps its own policy.
    const settings = await server.request('GET', '/api/settings');
    expect(settings.headers.get('content-security-policy')).toMatch(/script-src 'self'/);
  });
});
