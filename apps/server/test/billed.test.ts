import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { generateText, type LanguageModelUsage, streamText } from 'ai';
import type { ConversationSummary, OpenRouterHosts, ProviderCredential } from '@hearth/shared';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createLlmGateway } from '../src/llm.ts';
import { OpenRouterRouting } from '../src/openrouter.ts';
import { billedCost, callUsage, usageMetadata } from '../src/pricing.ts';
import { createTestServer } from './helpers.ts';

/** A stand-in for OpenRouter that bills like it: the cost comes with the usage. */
let server: http.Server;
let baseUrl = '';
const USAGE = {
  prompt_tokens: 1_000,
  completion_tokens: 200,
  total_tokens: 1_200,
  cost: 0.000224,
  cost_details: { upstream_inference_prompt_cost: 0.00014, upstream_inference_completions_cost: 0.000084 },
};

beforeAll(async () => {
  server = http.createServer((request, response) => {
    let body = '';
    request.on('data', (chunk) => (body += chunk));
    request.on('end', () => {
      const stream = (JSON.parse(body) as { stream?: boolean }).stream;
      const message = { id: 'gen-1', object: 'chat.completion', created: 0, model: 'deepseek/deepseek-v4.1-flash', provider: 'DeepInfra' };
      if (!stream) {
        response.writeHead(200, { 'content-type': 'application/json' });
        response.end(JSON.stringify({ ...message, choices: [{ index: 0, message: { role: 'assistant', content: 'Titel' }, finish_reason: 'stop' }], usage: USAGE }));
        return;
      }
      response.writeHead(200, { 'content-type': 'text/event-stream' });
      const chunk = (choice: object, usage?: object) =>
        `data: ${JSON.stringify({ ...message, object: 'chat.completion.chunk', choices: [{ index: 0, ...choice }], ...(usage ? { usage } : {}) })}\n\n`;
      response.write(chunk({ delta: { role: 'assistant', content: 'Hallo' }, finish_reason: null }));
      response.write(chunk({ delta: {}, finish_reason: 'stop' }, USAGE));
      response.end('data: [DONE]\n\n');
    });
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  baseUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}/api/v1`;
});

afterAll(async () => {
  await new Promise((resolve) => server.close(resolve));
});

const credential = () => ({ id: 'c1', kind: 'openrouter' as const, baseUrl, apiKey: 'sk-or-test' });

describe('billed cost', () => {
  it('comes from OpenRouter with every streamed answer', async () => {
    const result = streamText({ model: createLlmGateway().createModel(credential(), 'deepseek/deepseek-v4.1-flash'), prompt: 'Hallo?' });
    let billed: ReturnType<typeof billedCost> = null;
    for await (const part of result.stream) {
      if (part.type === 'finish-step') {
        billed = billedCost(part.providerMetadata);
      }
    }
    // With the host OpenRouter routed to, which every chunk names.
    expect(billed).toEqual({ total: 0.000224, input: 0.00014, output: 0.000084, host: 'DeepInfra' });
  });

  it('comes with answers that are not streamed, such as the title', async () => {
    const { providerMetadata } = await generateText({ model: createLlmGateway().createModel(credential(), 'deepseek/deepseek-v4.1-flash'), prompt: 'Titel?' });
    expect(billedCost(providerMetadata)).toMatchObject({ total: 0.000224, host: 'DeepInfra' });
  });

  it('replaces the estimate from catalog prices, call by call and for the reply', () => {
    const step = {
      inputTokens: 1_000,
      inputTokenDetails: { noCacheTokens: 1_000, cacheReadTokens: 0, cacheWriteTokens: 0 },
      outputTokens: 200,
      outputTokenDetails: { textTokens: 200, reasoningTokens: 0 },
      totalTokens: 1_200,
    } as LanguageModelUsage;
    // The catalog says 0.024 $ per million input tokens; OpenRouter billed more.
    const catalog = { input: 0.024, output: 0.096 };
    const estimated = callUsage(step, catalog, []);
    expect(estimated.billed).toBeUndefined();
    // Each call keeps the prices it ran at: the catalog's for an estimate…
    expect(estimated.rates).toEqual(catalog);
    const billed = callUsage(step, catalog, [], { total: 0.000224, input: 0.00014, output: 0.000084, host: 'DeepInfra' });
    expect(billed).toMatchObject({ billed: true, host: 'DeepInfra', cost: { total: 0.000224, input: 0.00014, output: 0.000084 } });
    expect(billed.cost).not.toHaveProperty('host');
    // …and the host's list price for a billed call, never the catalog's.
    expect(billed.rates).toBeUndefined();
    const listed = callUsage(step, catalog, [], { total: 0.000224, host: 'DeepInfra' }, (host) => (host === 'DeepInfra' ? { input: 0.14, output: 0.28 } : null));
    expect(listed.rates).toEqual({ input: 0.14, output: 0.28 });

    const usage = usageMetadata(step, step, catalog, [billed, billed]);
    expect(usage.cost).toBeCloseTo(0.000448, 9);
    expect(usage.billed).toBe(true);
    expect(usage.rates).toBeUndefined();
    // One call without a bill: the whole reply falls back to the estimate.
    const mixed = usageMetadata(step, step, catalog, [billed, estimated]);
    expect(mixed.billed).toBeUndefined();
    expect(mixed.rates).toEqual(catalog);
    // Other providers send no bill.
    expect(billedCost({ anthropic: { cacheCreationInputTokens: 0 } })).toBeNull();
    expect(billedCost(undefined)).toBeNull();
  });
});

describe('OpenRouter hosts in an ordinary chat', () => {
  it('follow the host the call before ran on, within a reply', async () => {
    const bodies: Record<string, unknown>[] = [];
    // First a tool call that OpenRouter served from the dearer host, then the answer.
    const standIn = http.createServer((request, response) => {
      let body = '';
      request.on('data', (chunk) => (body += chunk));
      request.on('end', () => {
        const parsed = JSON.parse(body) as Record<string, unknown>;
        bodies.push(parsed);
        // The title of the new chat comes without tools and is not streamed.
        if (!parsed.stream) {
          response.writeHead(200, { 'content-type': 'application/json' });
          response.end(JSON.stringify({ id: 'gen-t', object: 'chat.completion', created: 0, model: 'x', choices: [{ index: 0, message: { role: 'assistant', content: 'Rechnen' }, finish_reason: 'stop' }] }));
          return;
        }
        const first = bodies.filter((entry) => entry.stream).length === 1;
        const base = { id: `gen-${bodies.length}`, object: 'chat.completion.chunk', created: 0, model: 'deepseek/deepseek-v4.1-flash', provider: 'NoCache' };
        const delta = first
          ? { role: 'assistant', tool_calls: [{ index: 0, id: 'call-1', type: 'function', function: { name: 'calculate', arguments: '{"expressions":["1+1"]}' } }] }
          : { role: 'assistant', content: 'Zwei.' };
        response.writeHead(200, { 'content-type': 'text/event-stream' });
        response.write(`data: ${JSON.stringify({ ...base, choices: [{ index: 0, delta, finish_reason: null }] })}\n\n`);
        const usage = { prompt_tokens: 100, completion_tokens: 10, total_tokens: 110, cost: 0.00001 };
        response.write(`data: ${JSON.stringify({ ...base, choices: [{ index: 0, delta: {}, finish_reason: first ? 'tool_calls' : 'stop' }], usage })}\n\n`);
        response.end('data: [DONE]\n\n');
      });
    });
    await new Promise<void>((resolve) => standIn.listen(0, '127.0.0.1', resolve));
    const server = await createTestServer('sqlite', {
      // The real gateway, talking to the stand-in.
      llm: Object.assign(createLlmGateway(), { createdModels: [], listError: null }),
      openRouter: new OpenRouterRouting(
        async () => ({
          data: {
            endpoints: [
              { tag: 'cached/fp8', provider_name: 'Cached', pricing: { prompt: '0.0000001', completion: '0.0000002', input_cache_read: '0.00000001' } },
              { tag: 'no-cache', provider_name: 'NoCache', pricing: { prompt: '0.00000004', completion: '0.0000002' } },
            ],
          },
        }),
        Date.now,
        async () => ({ data: [{ slug: 'cached', headquarters: 'US', datacenters: null }] }),
      ),
    });
    try {
      const cookie = await server.signUp('owner@example.com');
      const credential = (await (
        await server.request('POST', '/api/providers', { cookie, body: { kind: 'openrouter', label: 'OpenRouter', apiKey: 'sk-or-0123456789abcdef' } })
      ).json()) as ProviderCredential;
      const port = (standIn.address() as AddressInfo).port;
      await server.database.db
        .updateTable('providerCredential')
        .set({ baseUrl: `http://127.0.0.1:${port}/api/v1` })
        .where('id', '=', credential.id)
        .execute();
      const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
      const reply = await server.request('POST', `/api/conversations/${id}/chat`, {
        cookie,
        body: {
          message: { id: 'm-1', role: 'user', parts: [{ type: 'text', text: 'Was ist 1 + 1?' }] },
          credentialId: credential.id,
          modelId: 'deepseek/deepseek-v4.1-flash',
        },
      });
      expect(await reply.text()).toContain('Zwei.');
      const chatCalls = bodies.filter((body) => Array.isArray(body.tools));
      expect(chatCalls).toHaveLength(2);
      expect(chatCalls[0]?.provider).toEqual({ order: ['cached/fp8', 'no-cache'], allow_fallbacks: true });
      // The cache of the conversation is where the first call ran.
      expect(chatCalls[1]?.provider).toEqual({ order: ['no-cache', 'cached/fp8'], allow_fallbacks: true });
      // One cache key for every call of the chat.
      expect(chatCalls[0]?.prompt_cache_key).toMatch(/^hearth-[0-9a-f]{32}$/);
      expect(chatCalls[1]?.prompt_cache_key).toBe(chatCalls[0]?.prompt_cache_key);

      // The user pins the model to a host: the chat goes there and nowhere else.
      const model = 'deepseek/deepseek-v4.1-flash';
      const listed = (await (await server.request('GET', `/api/openrouter/hosts?model=${encodeURIComponent(model)}`, { cookie })).json()) as OpenRouterHosts;
      expect(listed.pinned).toBeNull();
      expect(listed.hosts?.map((host) => [host.slug, host.headquarters])).toEqual([
        ['cached', 'US'],
        ['no-cache', null],
      ]);
      expect((await server.request('PUT', '/api/openrouter/hosts', { cookie, body: { model, host: 'elsewhere' } })).status).toBe(400);
      const pinned = (await (await server.request('PUT', '/api/openrouter/hosts', { cookie, body: { model, host: 'no-cache' } })).json()) as OpenRouterHosts;
      expect(pinned.pinned).toBe('no-cache');
      const again = await server.request('POST', `/api/conversations/${id}/chat`, {
        cookie,
        body: { message: { id: 'm-2', role: 'user', parts: [{ type: 'text', text: 'Und 2 + 2?' }] }, credentialId: credential.id, modelId: model },
      });
      expect(await again.text()).toContain('Zwei.');
      const pinnedCalls = bodies.filter((body) => Array.isArray(body.tools)).slice(2);
      expect(pinnedCalls).toHaveLength(1);
      expect(pinnedCalls[0]?.provider).toEqual({ order: ['no-cache'], allow_fallbacks: false });
      // Another user's pin is their own.
      const other = await server.signUp('other@example.com');
      const theirs = (await (await server.request('GET', `/api/openrouter/hosts?model=${encodeURIComponent(model)}`, { cookie: other })).json()) as OpenRouterHosts;
      expect(theirs.pinned).toBeNull();
      // Unpinned, it routes to the cheapest again.
      expect(((await (await server.request('PUT', '/api/openrouter/hosts', { cookie, body: { model, host: null } })).json()) as OpenRouterHosts).pinned).toBeNull();
    } finally {
      await server.close();
      await new Promise((resolve) => standIn.close(resolve));
    }
  });
});
