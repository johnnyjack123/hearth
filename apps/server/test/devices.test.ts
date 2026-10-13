import { createServer, type IncomingHttpHeaders, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import type {
  DeviceCredential,
  DeviceSelf,
  DeviceSummary,
  PairingInfo,
  PairingPoll,
  PairingStart,
  ProviderCredential,
} from '@harness/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createTestServer, DIALECTS, type TestServer } from './helpers.ts';

// Devices are command-line clients: no Origin header, no cookie.
const CLI = { origin: '' };

describe.each(DIALECTS)('devices on %s', (kind) => {
  let server: TestServer;
  let cookie: string;

  beforeEach(async () => {
    server = await createTestServer(kind);
    cookie = await server.signUp('owner@example.com');
  });

  afterEach(async () => {
    await server.close();
  });

  async function start(name = 'harness auf TEST'): Promise<PairingStart> {
    const response = await server.request('POST', '/api/pairing', { body: { name }, headers: CLI });
    expect(response.status).toBe(201);
    return (await response.json()) as PairingStart;
  }

  async function poll(deviceCode: string): Promise<PairingPoll> {
    const response = await server.request('POST', '/api/pairing/token', { body: { deviceCode }, headers: CLI });
    expect(response.status).toBe(200);
    return (await response.json()) as PairingPoll;
  }

  async function decide(userCode: string, approve: boolean, as = cookie): Promise<Response> {
    return server.request('POST', `/api/devices/pairing/${userCode}`, { cookie: as, body: { approve } });
  }

  async function pair(): Promise<{ token: string; deviceId: string }> {
    const pairing = await start();
    expect((await decide(pairing.userCode, true)).status).toBe(200);
    const result = await poll(pairing.deviceCode);
    if (result.status !== 'approved') {
      throw new Error(`pairing not approved: ${result.status}`);
    }
    return { token: result.token, deviceId: result.deviceId };
  }

  function asDevice(token: string) {
    return { ...CLI, authorization: `Bearer ${token}` };
  }

  it('pairs a device after confirmation and hands out the token once', async () => {
    const pairing = await start();
    expect(pairing.userCode).toMatch(/^[A-Z2-9]{4}-[A-Z2-9]{4}$/);
    expect(pairing.verificationUriComplete).toBe(`http://localhost:8080/device?code=${pairing.userCode}`);
    expect(await poll(pairing.deviceCode)).toEqual({ status: 'pending' });

    // Typed by a person: lower case, without the dash.
    const typed = pairing.userCode.toLowerCase().replace('-', '');
    const info = await server.request('GET', `/api/devices/pairing/${typed}`, { cookie });
    expect((await info.json()) as PairingInfo).toMatchObject({ name: 'harness auf TEST', status: 'pending' });
    expect((await decide(typed, true)).status).toBe(200);

    const approved = await poll(pairing.deviceCode);
    expect(approved.status).toBe('approved');
    expect(approved.status === 'approved' && approved.token).toMatch(/^hd_[\w-]{43}$/);
    // The token is gone from the server after the first pickup.
    expect(await poll(pairing.deviceCode)).toEqual({ status: 'expired' });
    expect((await decide(typed, true)).status).toBe(400);

    const devices = (await (await server.request('GET', '/api/devices', { cookie })).json()) as DeviceSummary[];
    expect(devices).toHaveLength(1);
    expect(devices[0]).toMatchObject({ name: 'harness auf TEST', lastUsedAt: null });
  });

  it('never hands out a token for denied, expired or unknown pairings', async () => {
    const denied = await start();
    expect((await decide(denied.userCode, false)).status).toBe(200);
    expect(await poll(denied.deviceCode)).toEqual({ status: 'denied' });

    const expired = await start();
    await server.database.db.updateTable('devicePairing').set({ expiresAt: Date.now() - 1 }).execute();
    expect(await poll(expired.deviceCode)).toEqual({ status: 'expired' });
    expect((await decide(expired.userCode, true)).status).toBe(400);

    expect(await poll('x'.repeat(43))).toEqual({ status: 'expired' });
    expect((await server.request('GET', '/api/devices/pairing/AAAA-AAAA', { cookie })).status).toBe(404);
    // Deciding needs a signed-in user.
    const anonymous = await start();
    expect((await server.request('POST', `/api/devices/pairing/${anonymous.userCode}`, { body: { approve: true } })).status).toBe(401);
  });

  it('accepts device tokens only on device and LLM routes', async () => {
    const { token } = await pair();
    const self = await server.request('GET', '/api/device', { headers: asDevice(token) });
    expect(self.status).toBe(200);
    expect(((await self.json()) as DeviceSelf).user.email).toBe('owner@example.com');

    // A leaked token must not open the account itself.
    for (const path of ['/api/conversations', '/api/providers', '/api/devices']) {
      expect((await server.request('GET', path, { headers: asDevice(token) })).status, path).toBe(401);
    }
    expect((await server.request('GET', '/api/device', { headers: asDevice('hd_wrong') })).status).toBe(401);
    expect((await server.request('GET', '/api/device', { cookie })).status).toBe(401);
  });

  it('revokes devices from the web app and from the device itself', async () => {
    const first = await pair();
    const second = await pair();
    expect((await server.request('DELETE', `/api/devices/${first.deviceId}`, { cookie })).status).toBe(204);
    expect((await server.request('GET', '/api/device', { headers: asDevice(first.token) })).status).toBe(401);

    // CLI logout: no body, no content type, no origin.
    expect((await server.request('DELETE', '/api/device', { headers: asDevice(second.token) })).status).toBe(204);
    expect((await server.request('GET', '/api/device', { headers: asDevice(second.token) })).status).toBe(401);

    const other = await server.signUp('other@example.com');
    const third = await pair();
    expect((await server.request('DELETE', `/api/devices/${third.deviceId}`, { cookie: other })).status).toBe(404);
  });

  it('keeps CSRF protection on session routes', async () => {
    const pairing = await start();
    const crossSite = await server.request('POST', `/api/devices/pairing/${pairing.userCode}`, {
      cookie,
      body: { approve: true },
      headers: { origin: 'https://evil.example', 'content-type': 'text/plain' },
    });
    expect(crossSite.status).toBe(403);
  });

  describe('LLM pass-through', () => {
    interface Seen {
      method: string;
      url: string;
      headers: IncomingHttpHeaders;
      body: string;
    }
    let provider: Server;
    let seen: Seen[];
    let providerUrl: string;

    beforeEach(async () => {
      seen = [];
      provider = createServer((req, res) => {
        let body = '';
        req.on('data', (chunk: Buffer) => {
          body += chunk.toString('utf8');
        });
        req.on('end', () => {
          seen.push({ method: req.method ?? '', url: req.url ?? '', headers: req.headers, body });
          res.writeHead(200, { 'content-type': 'text/event-stream', 'x-ratelimit-remaining': '42', 'set-cookie': 'a=b' });
          res.write('data: {"choices":[{"delta":{"content":"Hal"}}]}\n\n');
          res.end('data: [DONE]\n\n');
        });
      });
      await new Promise<void>((resolve) => provider.listen(0, '127.0.0.1', resolve));
      providerUrl = `http://127.0.0.1:${(provider.address() as AddressInfo).port}`;
    });

    afterEach(async () => {
      await new Promise<void>((resolve) => provider.close(() => resolve()));
    });

    async function credential(body: Record<string, unknown>): Promise<ProviderCredential> {
      const response = await server.request('POST', '/api/providers', { cookie, body });
      expect(response.status).toBe(201);
      return (await response.json()) as ProviderCredential;
    }

    it('forwards OpenAI-style requests with the stored key and streams the answer', async () => {
      const deepseek = await credential({
        kind: 'openai-compatible',
        label: 'DeepSeek',
        baseUrl: `${providerUrl}/v1`,
        apiKey: 'sk-real-key-0123456789',
      });
      const { token } = await pair();
      const response = await server.request('POST', `/api/llm/proxy/${deepseek.id}/chat/completions?stream=1`, {
        body: { model: 'deepseek-chat', stream: true },
        headers: { ...asDevice(token), 'x-api-key': 'agent-made-up', cookie: 'harness.session=stolen' },
      });
      expect(response.status).toBe(200);
      expect(response.headers.get('content-type')).toBe('text/event-stream');
      expect(response.headers.get('x-ratelimit-remaining')).toBe('42');
      expect(response.headers.get('set-cookie')).toBeNull();
      expect(await response.text()).toContain('[DONE]');

      expect(seen).toHaveLength(1);
      expect(seen[0]).toMatchObject({ method: 'POST', url: '/v1/chat/completions?stream=1' });
      expect(seen[0]?.headers.authorization).toBe('Bearer sk-real-key-0123456789');
      expect(seen[0]?.headers['x-api-key']).toBeUndefined();
      expect(seen[0]?.headers.cookie).toBeUndefined();
      expect(JSON.parse(seen[0]?.body ?? '')).toEqual({ model: 'deepseek-chat', stream: true });

      const devices = (await (await server.request('GET', '/api/devices', { cookie })).json()) as DeviceSummary[];
      expect(devices[0]?.lastUsedAt).toBeGreaterThan(0);
    });

    it('maps Anthropic paths and sends the key as x-api-key', async () => {
      const anthropic = await credential({
        kind: 'anthropic',
        label: 'Claude',
        baseUrl: `${providerUrl}/v1`,
        apiKey: 'sk-ant-real-0123456789',
      });
      const { token } = await pair();
      const response = await server.request('POST', `/api/llm/proxy/${anthropic.id}/v1/messages`, {
        body: { model: 'claude', messages: [] },
        headers: { ...asDevice(token), 'anthropic-version': '2023-06-01' },
      });
      expect(response.status).toBe(200);
      expect(seen[0]?.url).toBe('/v1/messages');
      expect(seen[0]?.headers['x-api-key']).toBe('sk-ant-real-0123456789');
      expect(seen[0]?.headers['anthropic-version']).toBe('2023-06-01');
      expect(seen[0]?.headers.authorization).toBeUndefined();
    });

    it('refuses other endpoints, foreign credentials and missing tokens', async () => {
      const mine = await credential({ kind: 'openai-compatible', label: 'Mine', baseUrl: providerUrl });
      const { token } = await pair();
      for (const path of ['files', 'chat/completions/../../admin', 'fine_tuning/jobs', 'v1/messages']) {
        const response = await server.request('POST', `/api/llm/proxy/${mine.id}/${path}`, {
          body: {},
          headers: asDevice(token),
        });
        expect(response.status, path).toBe(404);
      }
      const other = await server.signUp('other@example.com');
      const foreign = await server.request('POST', '/api/providers', {
        cookie: other,
        body: { kind: 'openai', label: 'Theirs', apiKey: 'sk-theirs-0123456789' },
      });
      const theirs = (await foreign.json()) as ProviderCredential;
      const stolen = await server.request('POST', `/api/llm/proxy/${theirs.id}/chat/completions`, {
        body: {},
        headers: asDevice(token),
      });
      expect(stolen.status).toBe(404);
      const anonymous = await server.request('POST', `/api/llm/proxy/${mine.id}/chat/completions`, {
        body: {},
        headers: CLI,
      });
      expect(anonymous.status).toBe(401);
      expect(seen).toHaveLength(0);
    });

    it('lists credentials with models only for user-run endpoints', async () => {
      await credential({ kind: 'openai', label: 'OpenAI', apiKey: 'sk-openai-0123456789' });
      await credential({ kind: 'openai-compatible', label: 'DeepSeek', baseUrl: providerUrl, apiKey: 'sk-x-0123456789' });
      const { token } = await pair();
      const response = await server.request('GET', '/api/llm/providers', { headers: asDevice(token) });
      const list = (await response.json()) as DeviceCredential[];
      expect(list.map((c) => [c.kind, c.label, c.models?.length ?? null])).toEqual([
        ['openai', 'OpenAI', null],
        ['openai-compatible', 'DeepSeek', 1],
      ]);
      expect(JSON.stringify(list)).not.toContain('sk-');
    });
  });
});
