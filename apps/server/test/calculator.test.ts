import type { CalculatorOutput, ConversationDetail, ConversationSummary, ProviderCredential } from '@harness/shared';
import { afterAll, afterEach, beforeEach, describe, expect, it } from 'vitest';
import { Calculator, CalculatorError } from '../src/calculator/engine.ts';
import { calculatorText } from '../src/calculator/tool.ts';
import type { ChatMessage } from '../src/services/conversations.ts';
import { createScriptedLlm, createTestServer, ORIGIN, readChunks, type Step, type TestServer } from './helpers.ts';

describe('Calculator', () => {
  const calculator = new Calculator();
  afterAll(() => calculator.close());

  it('calculates with numbers, units and shared variables', async () => {
    const results = await calculator.evaluate([
      '2^64',
      '0.1 + 0.2',
      'bignumber("0.1") + bignumber("0.2")',
      'fraction(1, 3) + fraction(1, 6)',
      '5 km + 300 m to mi',
      'r = 4 cm',
      'pi * r^2',
      'det([1, 2; 3, 4])',
      'combinations(49, 6)',
      'derivative("x^2 + 3x", "x")',
      'sqrt(-4)',
    ]);
    expect(results.map((entry) => entry.result)).toEqual([
      '1.844674407371e+19',
      '0.3',
      '0.3',
      '1/2',
      '3.2932673188579 mi',
      '4 cm',
      '50.265482457437 cm^2',
      '-2',
      '13983816',
      '2 * x + 3',
      '2i',
    ]);
  });

  it('reports errors per expression', async () => {
    const results = await calculator.evaluate(['1 / ', 'unknownFunction(2)', 'createUnit("foo")', '1 + 1']);
    expect(results.slice(0, 2).every((entry) => entry.error !== undefined && entry.result === undefined)).toBe(true);
    expect(results[3]).toEqual({ expression: '1 + 1', result: '2' });
    const lines = calculatorText({ results: [results[1], results[3]].filter((entry) => entry !== undefined) });
    expect(lines).toMatch(/^unknownFunction\(2\) → error: .*\n1 \+ 1 = 2$/);
  });

  it('keeps one call from changing the math of the next', async () => {
    // A page could ask the model to do this; the next chat must not notice.
    await calculator.evaluate(['config({number: "BigNumber"})', 'createUnit("blip", "3 m")', 'import({pi: 3}, {override: true})']);
    const after = await calculator.evaluate(['0.1 + 0.2', 'pi', '2 blip']);
    expect(after[0]?.result).toBe('0.3');
    expect(after[1]?.result).toBe('3.1415926535898');
    expect(after[2]?.error).toBeDefined();
  });

  it('stops calculations that take too long and goes on afterwards', async () => {
    const slow = new Calculator({ timeoutMs: 300 });
    try {
      await expect(slow.evaluate(['f(n) = n < 2 ? n : f(n - 1) + f(n - 2)', 'f(40)'])).rejects.toMatchObject({ code: 'timeout' });
      await expect(slow.evaluate(['6 * 7'])).resolves.toEqual([{ expression: '6 * 7', result: '42' }]);
    } finally {
      slow.close();
    }
  });

  it('stops calculations that take too much memory', async () => {
    const small = new Calculator({ timeoutMs: 20_000, memoryMb: 64 });
    try {
      const failed = await small.evaluate(['ones(20000, 20000)']).catch((error: unknown) => error);
      expect(failed).toBeInstanceOf(CalculatorError);
      expect((failed as CalculatorError).code).toBe('memory');
      await expect(small.evaluate(['1 + 1'])).resolves.toEqual([{ expression: '1 + 1', result: '2' }]);
    } finally {
      small.close();
    }
  }, 30_000);
});

describe('calculator in a chat', () => {
  let server: TestServer;
  let llm: ReturnType<typeof createScriptedLlm>;
  let script: Step[] = [];
  let cookie: string;
  let credential: ProviderCredential;

  beforeEach(async () => {
    llm = createScriptedLlm(() => script);
    server = await createTestServer('sqlite', { llm });
    cookie = await server.signUp('alice@example.com');
    credential = (await (
      await server.request('POST', '/api/providers', {
        cookie,
        body: { kind: 'openai', label: 'Work', apiKey: 'sk-test-0123456789abcdef' },
      })
    ).json()) as ProviderCredential;
  });

  afterEach(async () => {
    await server.close();
  });

  async function chat(conversationId: string, text: string) {
    const response = await server.app.request(`/api/conversations/${conversationId}/chat`, {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/json' },
      body: JSON.stringify({
        message: { id: `m-${Math.random().toString(36).slice(2)}`, role: 'user', parts: [{ type: 'text', text }] },
        credentialId: credential.id,
        modelId: 'mock-1',
      }),
    });
    expect(response.status).toBe(200);
    return readChunks(response);
  }

  it('gives the model the calculator and stores what it calculated', async () => {
    const created = await server.request('POST', '/api/conversations', { cookie, body: {} });
    const { id } = (await created.json()) as ConversationSummary;
    script = [{ tool: 'calculate', input: { expressions: ['19 / 100 * 250', '250 - 47.5'] } }, { text: 'Es sind 202,50 €.' }];
    await chat(id, 'Was kostet es nach 19 % Rabatt auf 250 €?');

    const first = llm.calls[0];
    expect(first?.tools).toContain('calculate');
    expect(first?.system).toContain('use the calculate tool');
    // The model gets the results as text after its call.
    expect(JSON.stringify(llm.calls[1]?.prompt)).toContain('19 / 100 * 250 = 47.5');

    const detail = (await (await server.request('GET', `/api/conversations/${id}`, { cookie })).json()) as ConversationDetail<ChatMessage>;
    const part = detail.messages[1]?.parts.find((candidate) => candidate.type === 'tool-calculate') as
      | { state: string; output: CalculatorOutput }
      | undefined;
    expect(part?.state).toBe('output-available');
    expect(part?.output.results).toEqual([
      { expression: '19 / 100 * 250', result: '47.5' },
      { expression: '250 - 47.5', result: '202.5' },
    ]);

    // Switched off by an admin: no tool, and the earlier call is text.
    const off = await server.request('PATCH', '/api/features', { cookie, body: { calculator: false } });
    expect(off.status).toBe(200);
    script = [{ text: 'ok' }];
    await chat(id, 'Danke');
    const last = llm.calls.at(-1);
    expect(last?.tools).not.toContain('calculate');
    expect(last?.system).not.toContain('calculate tool');
    expect(JSON.stringify(last?.prompt)).toContain('[Earlier, calculate: 19 / 100 * 250 = 47.5');
  });
});
