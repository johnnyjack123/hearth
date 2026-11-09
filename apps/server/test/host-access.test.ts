import http from 'node:http';
import type { AddressInfo } from 'node:net';
import type { ConversationSummary, HostCheckStatus, OpenRouterHostAccess, ProviderCredential } from '@harness/shared';
import { describe, expect, it } from 'vitest';
import { OpenRouterRouting } from '../src/openrouter.ts';
import { accessFromAnswer, HostProber, type HostAccessState, probeHost } from '../src/openrouter-access.ts';
import { blockedHosts } from '../src/services/host-access.ts';
import { createTestServer } from './helpers.ts';

const MODEL = 'deepseek/deepseek-v4.1-flash';
const KEY = { id: 'cred-1', apiKey: 'sk-or-test', baseUrl: 'https://openrouter.test/api/v1' };

/** A routing that lists two hosts for every model, without going online. */
function twoHosts() {
  return new OpenRouterRouting(
    async () => ({
      data: {
        endpoints: [
          { tag: 'deepinfra/fp8', provider_name: 'DeepInfra', pricing: { prompt: '0.0000001', completion: '0.0000002' } },
          { tag: 'deepseek', provider_name: 'DeepSeek', pricing: { prompt: '0.00000015', completion: '0.0000006' } },
        ],
      },
    }),
    Date.now,
    async () => null,
  );
}

describe('host probes', () => {
  it('read what an answer says about the host', () => {
    expect(accessFromAnswer(200, {})).toBe('allowed');
    expect(accessFromAnswer(404, { error: { message: 'No allowed providers are available for the selected model.', code: 404 } })).toBe('blocked');
    // The host itself refused: the request got there.
    expect(accessFromAnswer(400, { error: { message: 'max_tokens too small', metadata: { provider_name: 'DeepInfra' } } })).toBe('allowed');
    expect(accessFromAnswer(429, { error: { message: 'Rate limited' } })).toBe('unknown');
    expect(accessFromAnswer(500, null)).toBe('unknown');
  });

  it('ask each host once, a few at a time, and share a check that runs', async () => {
    let running = 0;
    let most = 0;
    const asked: string[] = [];
    const prober = new HostProber(async ({ host }) => {
      asked.push(host);
      running += 1;
      most = Math.max(most, running);
      await new Promise((resolve) => setTimeout(resolve, 5));
      running -= 1;
      return host.startsWith('ok') ? 'allowed' : 'blocked';
    });
    const hosts = Array.from({ length: 10 }, (_, index) => (index < 3 ? `ok-${index}` : `no-${index}`));
    const [states] = await Promise.all([prober.check(KEY, MODEL, hosts), prober.check(KEY, MODEL, hosts)]);
    expect(states.get('ok-0')).toBe('allowed');
    expect(states.get('no-5')).toBe('blocked');
    expect(asked).toHaveLength(10);
    expect(most).toBeLessThanOrEqual(6);
  });

  it('send a one-token request only the host may serve', async () => {
    const bodies: Record<string, unknown>[] = [];
    const standIn = http.createServer((request, response) => {
      let body = '';
      request.on('data', (chunk) => (body += chunk));
      request.on('end', () => {
        const parsed = JSON.parse(body) as { provider: { only: string[] } };
        bodies.push({ ...parsed, authorization: request.headers.authorization, path: request.url });
        const host = parsed.provider.only[0];
        response.writeHead(host === 'open' ? 200 : host === 'ignored' ? 404 : 503, { 'content-type': 'application/json' });
        response.end(JSON.stringify(host === 'open' ? { choices: [] } : { error: { message: 'x', code: 404 } }));
      });
    });
    await new Promise<void>((resolve) => standIn.listen(0, '127.0.0.1', resolve));
    try {
      const baseUrl = `http://127.0.0.1:${(standIn.address() as AddressInfo).port}/api/v1`;
      const results: HostAccessState[] = [];
      for (const host of ['open', 'ignored', 'busy']) {
        results.push(await probeHost({ apiKey: 'sk-or-test', baseUrl, model: MODEL, host }));
      }
      expect(results).toEqual(['allowed', 'blocked', 'unknown']);
      expect(bodies[0]).toMatchObject({
        path: '/api/v1/chat/completions',
        authorization: 'Bearer sk-or-test',
        model: MODEL,
        max_tokens: 1,
        provider: { only: ['open'], allow_fallbacks: false },
      });
    } finally {
      await new Promise((resolve) => standIn.close(resolve));
    }
  });

  it('keep the hosts an account rules out out of the cheapest order', async () => {
    const routing = twoHosts();
    expect(await routing.order(MODEL, { noFourBit: false })).toEqual(['deepinfra/fp8', 'deepseek']);
    expect(await routing.order(MODEL, { noFourBit: false, exclude: new Set(['deepinfra']) })).toEqual(['deepseek']);
  });
});

describe('host checks of an account', () => {
  async function setUp() {
    const asked: string[] = [];
    const server = await createTestServer('sqlite', {
      openRouter: twoHosts(),
      hostProber: new HostProber(async ({ host, model }) => {
        asked.push(`${model} ${host}`);
        return host === 'deepinfra' ? 'allowed' : 'blocked';
      }),
    });
    const cookie = await server.signUp('owner@example.com');
    const add = async (kind: string, as = cookie) =>
      (await (await server.request('POST', '/api/providers', { cookie: as, body: { kind, label: kind, apiKey: 'sk-or-0123456789abcdef' } })).json()) as ProviderCredential;
    const openRouter = await add('openrouter');
    // A chat on a model makes it one of the latest.
    const chatOn = async (modelId: string) => {
      const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
      await server.database.db.updateTable('conversation').set({ credentialId: openRouter.id, modelId, updatedAt: Date.now() }).where('id', '=', id).execute();
    };
    const access = async (model: string, as = cookie) =>
      server.request('GET', `/api/openrouter/access?model=${encodeURIComponent(model)}&credential=${openRouter.id}`, { cookie: as });
    return { server, cookie, openRouter, add, chatOn, access, asked };
  }

  it('checks a model of the latest chats once and keeps the result for a week', async () => {
    const { server, cookie, openRouter, chatOn, access, asked } = await setUp();
    try {
      // Not used in a chat yet: nothing is checked, nothing hidden.
      expect(await (await access(MODEL)).json()).toMatchObject({ checked: false, blocked: [] });
      expect(asked).toEqual([]);

      await chatOn(MODEL);
      const first = (await (await access(MODEL)).json()) as OpenRouterHostAccess;
      expect(first).toMatchObject({ checked: true, blocked: ['deepseek'], allowed: ['deepinfra'] });
      expect(asked).toHaveLength(2);
      // Asked again: from the database, no new probes.
      await access(MODEL);
      expect(asked).toHaveLength(2);
      const user = await server.database.db.selectFrom('user').select('id').executeTakeFirstOrThrow();
      expect(await blockedHosts(server.database.db, user.id, openRouter.id, MODEL)).toEqual(new Set(['deepseek']));

      // A week later it is checked again.
      await server.database.db.updateTable('openRouterAccess').set({ checkedAt: Date.now() - 8 * 24 * 60 * 60_000 }).execute();
      await access(MODEL);
      expect(asked).toHaveLength(4);

      // The settings card lists it; the button checks again right away.
      const status = (await (await server.request('GET', '/api/openrouter/access/status', { cookie })).json()) as HostCheckStatus;
      expect(status).toMatchObject({ enabled: true, models: [{ model: MODEL, blocked: 1, allowed: 1 }] });
      const refreshed = await server.request('POST', '/api/openrouter/access/refresh', { cookie });
      expect(refreshed.status).toBe(200);
      expect(asked).toHaveLength(6);
    } finally {
      await server.close();
    }
  });

  it('checks only the five latest models', async () => {
    const { server, chatOn, access, asked } = await setUp();
    try {
      await chatOn('vendor/oldest');
      for (let index = 0; index < 5; index += 1) {
        await chatOn(`vendor/model-${index}`);
      }
      expect(await (await access('vendor/oldest')).json()).toMatchObject({ checked: false });
      expect(asked).toEqual([]);
      await access('vendor/model-0');
      expect(asked).toHaveLength(2);
    } finally {
      await server.close();
    }
  });

  it('can be switched off, which also forgets the results', async () => {
    const { server, cookie, openRouter, chatOn, access, asked } = await setUp();
    try {
      await chatOn(MODEL);
      await access(MODEL);
      const off = (await (await server.request('PUT', '/api/openrouter/access/settings', { cookie, body: { enabled: false } })).json()) as HostCheckStatus;
      expect(off.enabled).toBe(false);
      expect(await (await access(MODEL)).json()).toMatchObject({ checked: false, disabled: true, blocked: [] });
      expect((await server.request('POST', '/api/openrouter/access/refresh', { cookie })).status).toBe(409);
      expect(asked).toHaveLength(2);
      const user = await server.database.db.selectFrom('user').select('id').executeTakeFirstOrThrow();
      expect(await blockedHosts(server.database.db, user.id, openRouter.id, MODEL)).toEqual(new Set());
      // Switched on again, the next look checks anew.
      await server.request('PUT', '/api/openrouter/access/settings', { cookie, body: { enabled: true } });
      expect(await (await access(MODEL)).json()).toMatchObject({ checked: true });
      expect(asked).toHaveLength(4);
    } finally {
      await server.close();
    }
  });

  it('answers only for the owner’s OpenRouter credentials', async () => {
    const { server, add, access } = await setUp();
    try {
      const openai = await add('openai');
      const wrongKind = await server.request('GET', `/api/openrouter/access?model=${encodeURIComponent(MODEL)}&credential=${openai.id}`, {
        cookie: await server.signUp('second@example.com'),
      });
      expect(wrongKind.status).toBe(404);
      const other = await server.signUp('other@example.com');
      expect((await access(MODEL, other)).status).toBe(404);
    } finally {
      await server.close();
    }
  });
});
