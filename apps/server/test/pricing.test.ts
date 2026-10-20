import type { ConversationDetail, ConversationSummary, ProviderCredential } from '@harness/shared';
import type { LanguageModelUsage } from 'ai';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { callUsage, catalogProvider, parseCatalog, PricingCatalog, usageMetadata } from '../src/pricing.ts';
import type { ChatMessage } from '../src/services/conversations.ts';
import { createTestServer, DIALECTS, eventually, type TestServer } from './helpers.ts';

function usage(input: number, output: number, cacheRead = 0): LanguageModelUsage {
  return {
    inputTokens: input,
    inputTokenDetails: { noCacheTokens: input - cacheRead, cacheReadTokens: cacheRead, cacheWriteTokens: 0 },
    outputTokens: output,
    outputTokenDetails: { textTokens: output, reasoningTokens: 0 },
    totalTokens: input + output,
  } as LanguageModelUsage;
}

describe('pricing', () => {
  it('maps credentials to catalog providers', () => {
    expect(catalogProvider('openai', null)).toBe('openai');
    expect(catalogProvider('openai', 'http://localhost:1234/v1')).toBeNull();
    expect(catalogProvider('openai-compatible', 'https://api.deepseek.com')).toBe('deepseek');
    expect(catalogProvider('openai-compatible', 'https://llm.example.com/v1')).toBeNull();
    expect(catalogProvider('ollama', 'http://localhost:11434/v1')).toBe('free');
  });

  it('keeps only usable fields of the catalog', () => {
    const catalog = parseCatalog({
      deepseek: {
        models: {
          'DeepSeek-Chat': { limit: { context: 128_000 }, cost: { input: 0.27, output: 1.1, cache_read: 0.07 } },
          broken: { limit: { context: 'many' }, cost: { input: -1 } },
        },
      },
      somewhere: { models: { x: {} } },
    });
    expect(catalog.get('deepseek')?.get('deepseek-chat')).toEqual({
      contextWindow: 128_000,
      input: 0.27,
      output: 1.1,
      cacheRead: 0.07,
    });
    expect(catalog.get('deepseek')?.get('broken')).toEqual({});
    expect(parseCatalog(null).size).toBe(0);
  });

  it('computes cost with cache prices and the context of the last call', () => {
    const pricing = { contextWindow: 1000, input: 2, output: 8, cacheRead: 0.5 };
    const result = usageMetadata(usage(300, 50, 100), usage(200, 30, 100), pricing);
    expect(result).toEqual({
      inputTokens: 300,
      outputTokens: 50,
      cachedInputTokens: 100,
      contextTokens: 230,
      contextWindow: 1000,
      cost: (200 * 2 + 100 * 0.5 + 50 * 8) / 1_000_000,
      rates: { input: 2, output: 8, cacheRead: 0.5 },
    });
    expect(usageMetadata(usage(10, 5), undefined, null)).toEqual({ inputTokens: 10, outputTokens: 5, contextTokens: 15 });
  });

  it('splits each model call into tokens and cost by kind', () => {
    const pricing = { input: 2, output: 8, cacheRead: 0.5 };
    const step = { ...usage(300, 50, 100), outputTokenDetails: { textTokens: 20, reasoningTokens: 30 } } as LanguageModelUsage;
    const call = callUsage(step, pricing, ['web_search']);
    expect(call).toEqual({
      input: 300,
      cacheRead: 100,
      cacheWrite: 0,
      output: 50,
      reasoning: 30,
      tools: ['web_search'],
      cost: {
        total: expect.closeTo((200 * 2 + 100 * 0.5 + 50 * 8) / 1_000_000, 12),
        input: (200 * 2) / 1_000_000,
        cacheRead: (100 * 0.5) / 1_000_000,
        cacheWrite: 0,
        output: (50 * 8) / 1_000_000,
      },
    });
    // The calls add up to the reply.
    const reply = usageMetadata(step, step, pricing, [call]);
    expect(reply.cost).toBeCloseTo(call.cost?.total ?? 0, 12);
    expect(reply.reasoningTokens).toBe(30);
    // Without prices only the tokens.
    expect(callUsage(step, null, []).cost).toBeUndefined();
  });

  it('answers without the catalog and does not retry at once', async () => {
    let calls = 0;
    const catalog = new PricingCatalog(async () => {
      calls += 1;
      throw new Error('offline');
    });
    expect(await catalog.lookup('openai', null, 'gpt-4o')).toBeNull();
    expect(await catalog.lookup('openai', null, 'gpt-4o')).toBeNull();
    expect(calls).toBe(1);
    expect(await catalog.lookup('ollama', null, 'llama3')).toMatchObject({ input: 0, output: 0 });
  });
});

describe.each(DIALECTS)('usage in chats on %s', (kind) => {
  let server: TestServer;
  let cookie: string;

  beforeEach(async () => {
    server = await createTestServer(kind);
    cookie = await server.signUp('owner@example.com');
  });

  afterEach(async () => {
    await server.close();
  });

  it('stores cost, context and window with the reply', async () => {
    const credential = (await (
      await server.request('POST', '/api/providers', {
        cookie,
        body: { kind: 'openai', label: 'Work', apiKey: 'sk-test-0123456789abcdef' },
      })
    ).json()) as ProviderCredential;
    const conversation = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    const response = await server.request('POST', `/api/conversations/${conversation.id}/chat`, {
      cookie,
      body: {
        message: { id: 'm1', role: 'user', parts: [{ type: 'text', text: 'Hallo' }] },
        credentialId: credential.id,
        modelId: 'mock-1',
      },
    });
    await response.text();
    const load = async () =>
      (await (await server.request('GET', `/api/conversations/${conversation.id}`, { cookie })).json()) as ConversationDetail<ChatMessage>;
    await eventually(async () => (await load()).messages.length === 2);
    // The mock model reports 5 input and 7 output tokens; the test catalog
    // prices mock-1 at 2 and 8 dollars per million.
    const cost = (5 * 2 + 7 * 8) / 1_000_000;
    expect((await load()).messages[1]?.metadata?.usage).toEqual({
      inputTokens: 5,
      outputTokens: 7,
      contextTokens: 12,
      contextWindow: 1000,
      cost,
      rates: { input: 2, output: 8 },
      calls: [
        {
          input: 5,
          cacheRead: 0,
          cacheWrite: 0,
          output: 7,
          tools: [],
          cost: { total: cost, input: (5 * 2) / 1_000_000, cacheRead: 0, cacheWrite: 0, output: (7 * 8) / 1_000_000 },
        },
      ],
    });
  });
});
