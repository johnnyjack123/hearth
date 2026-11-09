import { createHmac } from 'node:crypto';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createTestServer, DIALECTS, type TestServer } from './helpers.ts';

const PASSWORD = 'correct-horse-battery';

/** RFC 4648 base32, as in the `secret` of an otpauth URI. */
function base32(text: string): Buffer {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = '';
  for (const char of text.replace(/=+$/, '').toUpperCase()) {
    bits += alphabet.indexOf(char).toString(2).padStart(5, '0');
  }
  return Buffer.from(bits.match(/.{8}/g)?.map((byte) => Number.parseInt(byte, 2)) ?? []);
}

/** The code an authenticator app shows now (RFC 6238: 30 s, 6 digits, SHA-1). */
function totp(uri: string, now = Date.now()): string {
  const secret = new URL(uri).searchParams.get('secret') ?? '';
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(Math.floor(now / 30_000)));
  const mac = createHmac('sha1', base32(secret)).update(counter).digest();
  const offset = (mac.at(-1) ?? 0) & 0xf;
  return ((mac.readUInt32BE(offset) & 0x7fffffff) % 1_000_000).toString().padStart(6, '0');
}

const cookiesOf = (response: Response) =>
  response.headers
    .getSetCookie()
    .map((cookie) => cookie.split(';')[0])
    .join('; ');

describe.each(DIALECTS)('two-factor sign-in on %s', (kind) => {
  let server: TestServer;
  let cookie: string;

  beforeEach(async () => {
    server = await createTestServer(kind);
    cookie = await server.signUp('owner@example.com');
  });

  afterEach(async () => {
    await server.close();
  });

  const signIn = () => server.request('POST', '/api/auth/sign-in/email', { body: { email: 'owner@example.com', password: PASSWORD } });

  async function enable(): Promise<{ uri: string; backupCodes: string[] }> {
    const enabled = await server.request('POST', '/api/auth/two-factor/enable', { cookie, body: { password: PASSWORD } });
    expect(enabled.status).toBe(200);
    const { totpURI, backupCodes } = (await enabled.json()) as { totpURI: string; backupCodes: string[] };
    // Only the first code from the app switches it on.
    const verified = await server.request('POST', '/api/auth/two-factor/verify-totp', { cookie, body: { code: totp(totpURI) } });
    expect(verified.status).toBe(200);
    cookie = cookiesOf(verified) || cookie;
    return { uri: totpURI, backupCodes };
  }

  it('asks for the app’s code after the password once it is on', async () => {
    const { uri } = await enable();
    expect(uri).toMatch(/^otpauth:\/\/totp\/LLM%20Harness:owner%40example\.com\?/);

    const first = await signIn();
    expect(await first.json()).toMatchObject({ twoFactorRedirect: true });
    const pending = cookiesOf(first);
    // The password alone gives no session.
    expect((await server.request('GET', '/api/conversations', { cookie: pending })).status).toBe(401);
    expect((await server.request('POST', '/api/auth/two-factor/verify-totp', { cookie: pending, body: { code: '000000' } })).ok).toBe(false);
    const verified = await server.request('POST', '/api/auth/two-factor/verify-totp', { cookie: pending, body: { code: totp(uri) } });
    expect(verified.status).toBe(200);
    expect((await server.request('GET', '/api/conversations', { cookie: cookiesOf(verified) })).status).toBe(200);
  });

  it('takes a backup code once instead', async () => {
    const { backupCodes } = await enable();
    expect(backupCodes.length).toBeGreaterThanOrEqual(8);
    const code = backupCodes[0] ?? '';
    const pending = cookiesOf(await signIn());
    const used = await server.request('POST', '/api/auth/two-factor/verify-backup-code', { cookie: pending, body: { code } });
    expect(used.status).toBe(200);
    const again = cookiesOf(await signIn());
    expect((await server.request('POST', '/api/auth/two-factor/verify-backup-code', { cookie: again, body: { code } })).ok).toBe(false);
  });

  it('is switched off with the password, then the password is enough again', async () => {
    await enable();
    expect((await server.request('POST', '/api/auth/two-factor/disable', { cookie, body: { password: 'wrong-password-x' } })).ok).toBe(false);
    expect((await server.request('POST', '/api/auth/two-factor/disable', { cookie, body: { password: PASSWORD } })).status).toBe(200);
    const plain = await signIn();
    expect(await plain.json()).not.toHaveProperty('twoFactorRedirect');
  });
});
