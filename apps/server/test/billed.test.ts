import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { generateText, type LanguageModelUsage, streamText } from 'ai';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createLlmGateway } from '../src/llm.ts';
import { billedCost, callUsage, usageMetadata } from '../src/pricing.ts';

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
    const billed = callUsage(step, catalog, [], { total: 0.000224, input: 0.00014, output: 0.000084, host: 'DeepInfra' });
    expect(billed).toMatchObject({ billed: true, host: 'DeepInfra', cost: { total: 0.000224, input: 0.00014, output: 0.000084 } });
    expect(billed.cost).not.toHaveProperty('host');

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
