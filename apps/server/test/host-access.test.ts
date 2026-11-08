import http from 'node:http';
import type { AddressInfo } from 'node:net';
import type { OpenRouterHostAccess, ProviderCredential } from '@harness/shared';
import { describe, expect, it } from 'vitest';
import { OpenRouterRouting } from '../src/openrouter.ts';
import { accessFromAnswer, HostAccess, type HostAccessState, probeHost } from '../src/openrouter-access.ts';
import { createTestServer } from './helpers.ts';

const MODEL = 'deepseek/deepseek-v4.1-flash';
const KEY = { id: 'cred-1', apiKey: 'sk-or-test', baseUrl: 'https://openrouter.test/api/v1' };

describe('host access', () => {
  it('reads what a probe answer says about the host', () => {
    expect(accessFromAnswer(200, {})).toBe('allowed');
    expect(accessFromAnswer(404, { error: { message: 'No allowed providers are available for the selected model.', code: 404 } })).toBe('blocked');
    // The host itself refused: the request got there.
    expect(accessFromAnswer(400, { error: { message: 'max_tokens too small', metadata: { provider_name: 'DeepInfra' } } })).toBe('allowed');
    expect(accessFromAnswer(429, { error: { message: 'Rate limited' } })).toBe('unknown');
    expect(accessFromAnswer(500, null)).toBe('unknown');
  });

  it('probes each host once, a few at a time, and keeps the result', async () => {
    let now = 0;
    let running = 0;
    let most = 0;
    const asked: string[] = [];
    const access = new HostAccess(async ({ host }) => {
      asked.push(host);
      running += 1;
      most = Math.max(most, running);
      await new Promise((resolve) => setTimeout(resolve, 5));
      running -= 1;
      return host.startsWith('ok') ? 'allowed' : 'blocked';
    }, () => now);
    const hosts = Array.from({ length: 10 }, (_, index) => (index < 3 ? `ok-${index}` : `no-${index}`));
    const states = await access.check(KEY, MODEL, hosts);
    expect(states.get('ok-0')).toBe('allowed');
    expect(states.get('no-5')).toBe('blocked');
    expect(asked).toHaveLength(10);
    expect(most).toBeLessThanOrEqual(6);
    // A second look within 15 minutes asks nobody.
    await access.check(KEY, MODEL, hosts);
    expect(asked).toHaveLength(10);
    expect(access.blocked(KEY.id, MODEL)).toEqual(new Set(hosts.slice(3)));
    // Later the list is checked again; routing keeps the result for hours.
    now = 16 * 60_000;
    await access.check(KEY, MODEL, hosts);
    expect(asked).toHaveLength(20);
    now += 7 * 60 * 60_000;
    expect(access.blocked(KEY.id, MODEL)).toEqual(new Set());
    // Another credential has its own result.
    expect(access.blocked('cred-2', MODEL)).toEqual(new Set());
  });

  it('sends a one-token request only the host may serve', async () => {
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

  it('leaves the hosts an account rules out out of the cheapest order', async () => {
    const routing = new OpenRouterRouting(
      async () => ({
        data: {
          endpoints: [
            { tag: 'cheap/fp8', provider_name: 'Cheap', pricing: { prompt: '0.00000001', completion: '0.00000002' } },
            { tag: 'dear', provider_name: 'Dear', pricing: { prompt: '0.0000001', completion: '0.0000002' } },
          ],
        },
      }),
      Date.now,
      async () => null,
    );
    expect(await routing.order(MODEL, { noFourBit: false })).toEqual(['cheap/fp8', 'dear']);
    expect(await routing.order(MODEL, { noFourBit: false, exclude: new Set(['cheap']) })).toEqual(['dear']);
  });

  it('answers per credential on the API, for the owner only', async () => {
    const server = await createTestServer('sqlite', {
      openRouter: new OpenRouterRouting(
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
      ),
      hostAccess: new HostAccess(async ({ host, apiKey }) => (apiKey === 'sk-or-0123456789abcdef' && host === 'deepinfra' ? 'allowed' : 'blocked')),
    });
    try {
      const cookie = await server.signUp('owner@example.com');
      const add = async (kind: string, as = cookie) =>
        (await (await server.request('POST', '/api/providers', { cookie: as, body: { kind, label: kind, apiKey: 'sk-or-0123456789abcdef' } })).json()) as ProviderCredential;
      const openRouter = await add('openrouter');
      const access = await server.request('GET', `/api/openrouter/access?model=${encodeURIComponent(MODEL)}&credential=${openRouter.id}`, { cookie });
      expect((await access.json()) as OpenRouterHostAccess).toEqual({ blocked: ['deepseek'], allowed: ['deepinfra'], checked: true });

      // Another provider's key, or someone else's, says nothing.
      const openai = await add('openai');
      expect((await server.request('GET', `/api/openrouter/access?model=${encodeURIComponent(MODEL)}&credential=${openai.id}`, { cookie })).status).toBe(404);
      const other = await server.signUp('other@example.com');
      expect((await server.request('GET', `/api/openrouter/access?model=${encodeURIComponent(MODEL)}&credential=${openRouter.id}`, { cookie: other })).status).toBe(404);
      // A model variant has no list to check.
      const variant = await server.request('GET', `/api/openrouter/access?model=${encodeURIComponent(`${MODEL}:free`)}&credential=${openRouter.id}`, { cookie });
      expect(await variant.json()).toEqual({ blocked: [], allowed: [], checked: false });
    } finally {
      await server.close();
    }
  });
});
