import { describe, expect, it } from 'vitest';
import {
  AgentBills,
  cheapestProvider,
  hostRates,
  meterBill,
  OpenRouterRouting,
  rankEndpoints,
  withProviderOrder,
} from '../src/openrouter.ts';
import { agentCallCost, agentModelInfo, deepSeekPeak, pricesAt } from '../src/pricing.ts';

/** Parts of OpenRouter's endpoint list for DeepSeek V4.1 Flash on 1.11.2026. */
const ENDPOINTS = {
  data: {
    endpoints: [
      {
        tag: 'open-inference/fp4',
        quantization: 'fp4',
        status: 0,
        uptime_last_30m: 100,
        pricing: { prompt: '0.000000035', completion: '0.0000005', input_cache_read: '0.000000005', discount: 0 },
      },
      {
        tag: 'morph/fp8',
        quantization: 'fp8',
        status: 0,
        uptime_last_30m: 100,
        pricing: { prompt: '0.00000005', completion: '0.000000192', input_cache_read: '0.000000008', discount: 0 },
      },
      // Cheap on paper, but without a cache price every cached token costs full price.
      { tag: 'no-cache', quantization: 'fp8', status: 0, uptime_last_30m: 100, pricing: { prompt: '0.00000004', completion: '0.0000002' } },
      // Down: left out.
      { tag: 'fireworks/spot', status: -5, uptime_last_30m: 46, pricing: { prompt: '0.00000001', completion: '0.00000001' } },
      // The discounted flex tier (DeepInfra, 51 % off and 8 bit): left out as well.
      {
        tag: 'deepinfra/fp8/flex',
        service_tier: 'flex',
        quantization: 'fp8',
        status: 0,
        uptime_last_30m: 100,
        pricing: { prompt: '0.0000000784', completion: '0.0000002352', input_cache_read: '0.000000002352', discount: 0.51 },
      },
      {
        tag: 'deepseek',
        status: 0,
        uptime_last_30m: 100,
        pricing: {
          prompt: '0.00000015',
          completion: '0.0000006',
          input_cache_read: '0.000000003',
          overrides: [
            { utc_days: ['saturday', 'sunday'], prompt: '0.00000015', completion: '0.0000006', input_cache_read: '0.000000003' },
            {
              utc_start: 100,
              utc_end: 400,
              utc_days: ['monday', 'tuesday', 'wednesday', 'thursday', 'friday'],
              prompt: '0.0000003',
              completion: '0.0000012',
              input_cache_read: '0.000000006',
            },
          ],
        },
      },
    ],
  },
};

// Saturday noon and Monday 02:00 UTC.
const SATURDAY = new Date('2026-10-31T12:00:00Z');
const MONDAY_PEAK = new Date('2026-11-02T02:00:00Z');

describe('cheapest OpenRouter host', () => {
  it('compares hosts on a typical call with cached input, not on list prices', () => {
    const order = rankEndpoints(ENDPOINTS, SATURDAY, { noFourBit: false });
    // Morph's cheap output beats OpenInference's cheaper input; the host
    // without a cache price comes last although its list price is lowest.
    expect(order).toEqual(['morph/fp8', 'open-inference/fp4', 'deepseek', 'no-cache']);
    expect(order).not.toContain('fireworks/spot');
    expect(order).not.toContain('deepinfra/fp8/flex');
  });

  it('applies the hours of a host and can leave out 4-bit hosts', () => {
    const peak = rankEndpoints(ENDPOINTS, MONDAY_PEAK, { noFourBit: true });
    // DeepSeek doubles its prices from 01:00 to 04:00 UTC on weekdays.
    expect(peak).toEqual(['morph/fp8', 'no-cache', 'deepseek']);
    expect(rankEndpoints({ nothing: true }, SATURDAY, { noFourBit: false })).toEqual([]);
  });

  it('fetches a model list once and only for plain model ids', async () => {
    const asked: string[] = [];
    let now = 0;
    const routing = new OpenRouterRouting(
      async (id) => {
        asked.push(id);
        return ENDPOINTS;
      },
      () => now,
    );
    expect(await routing.order('deepseek/deepseek-v4.1-flash-1017', { noFourBit: false })).toHaveLength(4);
    expect(await routing.order('deepseek/deepseek-v4.1-flash-1017', { noFourBit: true })).toHaveLength(3);
    expect(await routing.order('deepseek/deepseek-v4.1-flash:free', { noFourBit: false })).toBeNull();
    expect(await routing.order('../../admin', { noFourBit: false })).toBeNull();
    expect(asked).toEqual(['deepseek/deepseek-v4.1-flash-1017']);
    now += 11 * 60_000;
    await routing.order('deepseek/deepseek-v4.1-flash-1017', { noFourBit: false });
    expect(asked).toHaveLength(2);
  });

  it('leaves a request alone that routes itself, and keeps other fields', () => {
    expect(cheapestProvider({ sort: 'throughput' }, ['a'])).toBeUndefined();
    expect(cheapestProvider({ order: ['x'] }, ['a'])).toBeUndefined();
    expect(cheapestProvider(undefined, null)).toBeUndefined();
    expect(cheapestProvider({ data_collection: 'deny' }, ['a', 'b'])).toEqual({
      data_collection: 'deny',
      order: ['a', 'b'],
      allow_fallbacks: true,
    });
    // Ordinary chats keep their thinking effort next to the order.
    expect(withProviderOrder({ providerOptions: { openrouter: { reasoning: { effort: 'high' } } } }, ['a'])).toEqual({
      providerOptions: { openrouter: { reasoning: { effort: 'high' }, provider: { order: ['a'], allow_fallbacks: true } } },
    });
    expect(withProviderOrder({ reasoning: 'low' }, null)).toEqual({ reasoning: 'low' });
  });
});

describe('prices of the host that ran a call', () => {
  // DeepInfra with a regular and a flex endpoint, DeepSeek with night prices.
  const LIST = {
    data: {
      endpoints: [
        {
          tag: 'deepinfra/fp8/flex',
          provider_name: 'DeepInfra',
          service_tier: 'flex',
          pricing: { prompt: '0.0000000784', completion: '0.0000002352', input_cache_read: '0.000000002352' },
        },
        {
          tag: 'deepinfra/fp8',
          provider_name: 'DeepInfra',
          pricing: { prompt: '0.00000016', completion: '0.00000048', input_cache_read: '0.0000000048' },
        },
        {
          tag: 'deepinfra/bf16',
          provider_name: 'DeepInfra',
          pricing: { prompt: '0.0000003', completion: '0.0000009' },
        },
        ...ENDPOINTS.data.endpoints.filter((entry) => entry.tag === 'deepseek').map((entry) => ({ ...entry, provider_name: 'DeepSeek' })),
      ],
    },
  };

  it('takes the host’s cheapest regular endpoint, per million tokens, at the time of the call', () => {
    // Flex is left out, as in the routing; of the two others the cheaper counts.
    expect(hostRates(LIST, 'DeepInfra', SATURDAY)).toEqual({ input: 0.16, output: 0.48, cacheRead: 0.0048 });
    expect(hostRates(LIST, 'DeepSeek', SATURDAY)).toEqual({ input: 0.15, output: 0.6, cacheRead: 0.003 });
    expect(hostRates(LIST, 'DeepSeek', MONDAY_PEAK)).toEqual({ input: 0.3, output: 1.2, cacheRead: 0.006 });
    expect(hostRates(LIST, 'Chutes', SATURDAY)).toBeNull();
    expect(hostRates(null, 'DeepInfra', SATURDAY)).toBeNull();
  });

  it('come from the list loaded for the model, and with the bill into the call', async () => {
    const routing = new OpenRouterRouting(async () => LIST, () => SATURDAY.getTime());
    expect(routing.rates('deepseek/deepseek-v4.1-flash', 'DeepInfra', SATURDAY)).toBeNull();
    await routing.preload('deepseek/deepseek-v4.1-flash');
    const rates = (host: string) => routing.rates('deepseek/deepseek-v4.1-flash', host, SATURDAY);
    expect(rates('DeepInfra')).toEqual({ input: 0.16, output: 0.48, cacheRead: 0.0048 });

    const call = { input: 100_000, cacheRead: 99_000, output: 1_000 };
    expect(agentCallCost(call, 'openrouter', null, { total: 0.0009, host: 'DeepInfra' }, rates)).toEqual({
      cost: { total: 0.0009 },
      billed: true,
      host: 'DeepInfra',
      rates: { input: 0.16, output: 0.48, cacheRead: 0.0048 },
    });
  });
});

describe('bills of agent calls', () => {
  const USAGE = {
    prompt_tokens: 1200,
    completion_tokens: 30,
    cost: 0.0004,
    cost_details: { upstream_inference_prompt_cost: 0.00025, upstream_inference_completions_cost: 0.00015 },
  };

  async function pass(parts: string[], stream: boolean) {
    const bills: unknown[] = [];
    const encoder = new TextEncoder();
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        for (const part of parts) {
          controller.enqueue(encoder.encode(part));
        }
        controller.close();
      },
    });
    const text = await new Response(meterBill(body, stream, (bill) => bills.push(bill))).text();
    return { text, bills };
  }

  it('reads the bill from the last chunk of a stream, also when lines are split', async () => {
    const last = `data: ${JSON.stringify({ provider: 'Chutes', choices: [], usage: USAGE })}\n\n`;
    const parts = ['data: {"choices":[{"delta":{"content":"Hallo"}}]}\n\n', last.slice(0, 40), last.slice(40), 'data: [DONE]\n\n'];
    const { text, bills } = await pass(parts, true);
    expect(text).toBe(parts.join(''));
    expect(bills).toEqual([{ cost: { total: 0.0004, input: 0.00025, output: 0.00015, host: 'Chutes' }, promptTokens: 1200, completionTokens: 30 }]);
  });

  it('reads the bill of a JSON answer, and none without a cost', async () => {
    expect((await pass([JSON.stringify({ usage: USAGE })], false)).bills).toHaveLength(1);
    expect((await pass([JSON.stringify({ usage: { prompt_tokens: 1, completion_tokens: 1 } })], false)).bills).toHaveLength(0);
    expect((await pass(['not json'], false)).bills).toHaveLength(0);
  });

  it('matches bills to the agent call with the same tokens, once', () => {
    let now = 0;
    const bills = new AgentBills(() => now);
    bills.record('d1', { cost: { total: 0.1 }, promptTokens: 100, completionTokens: 5 });
    bills.record('d1', { cost: { total: 0.2 }, promptTokens: 200, completionTokens: 5 });
    expect(bills.take('d2', 200, 5)).toBeNull();
    expect(bills.take('d1', 200, 5)).toEqual({ total: 0.2 });
    expect(bills.take('d1', 200, 5)).toBeNull();
    now += 16 * 60_000;
    expect(bills.take('d1', 100, 5)).toBeNull();
  });
});

describe('prices of agent calls', () => {
  // models.dev lists DeepSeek's off-peak prices.
  const flash = { input: 0.15, output: 0.6, cacheRead: 0.003, contextWindow: 1_048_576, name: 'DeepSeek V4 Flash', reasoning: true, images: true };

  it('knows DeepSeek peak hours: weekdays 01:00–04:00 and 06:00–10:00 UTC', () => {
    expect(deepSeekPeak(MONDAY_PEAK)).toBe(true);
    expect(deepSeekPeak(new Date('2026-11-02T09:59:00Z'))).toBe(true);
    expect(deepSeekPeak(new Date('2026-11-02T04:00:00Z'))).toBe(false);
    expect(deepSeekPeak(new Date('2026-11-02T15:30:00Z'))).toBe(false);
    expect(deepSeekPeak(new Date('2026-10-31T08:00:00Z'))).toBe(false);
    expect(pricesAt('deepseek', flash, MONDAY_PEAK)).toMatchObject({ input: 0.3, output: 1.2, cacheRead: 0.006 });
    expect(pricesAt('deepseek', flash, SATURDAY)).toBe(flash);
    expect(pricesAt('openai', flash, MONDAY_PEAK)).toBe(flash);
  });

  it('prices DeepSeek itself, takes bills over estimates and pi for the rest', () => {
    const call = { input: 100_000, cacheRead: 99_000, output: 1_000, agentCost: 0.0055 };
    // Off-peak: 1000 × 0.15 + 99000 × 0.003 + 1000 × 0.6 per million.
    const deepseek = agentCallCost(call, 'deepseek', flash, null);
    expect(deepseek.cost?.total).toBeCloseTo(0.001047, 9);
    expect(deepseek.billed).toBeUndefined();
    expect(agentCallCost(call, 'openrouter', null, { total: 0.0009 })).toEqual({ cost: { total: 0.0009 }, billed: true });
    expect(agentCallCost(call, 'openrouter', null, { total: 0.0009, host: 'DeepInfra' })).toEqual({ cost: { total: 0.0009 }, billed: true, host: 'DeepInfra' });
    expect(agentCallCost(call, 'anthropic', flash, null)).toEqual({ cost: { total: 0.0055 } });
    expect(agentCallCost({ ...call, agentCost: undefined }, 'deepseek', null, null)).toEqual({});
  });

  it('describes a model for a harness that does not know it', () => {
    expect(agentModelInfo(flash)).toEqual({
      name: 'DeepSeek V4 Flash',
      contextWindow: 1_048_576,
      reasoning: true,
      images: true,
      cost: { input: 0.15, output: 0.6, cacheRead: 0.003 },
    });
    expect(agentModelInfo(null)).toEqual({});
  });
});
