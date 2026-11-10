import type { ConversationDetail, ConversationSummary, CostStatus, ProviderCredential } from '@hearth/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { costGuard } from '../src/services/costs.ts';
import type { ChatMessage } from '../src/services/conversations.ts';
import { createScriptedLlm, createTestServer, DIALECTS, eventually, readChunks, type Step, type TestServer } from './helpers.ts';

// The test catalog prices mock-1 at 2 and 8 dollars per million tokens; each
// mock call reads 5 and writes 7 tokens: 0.000066 $.
const CALL = (5 * 2 + 7 * 8) / 1_000_000;

describe.each(DIALECTS)('daily cost limit on %s', (kind) => {
  let server: TestServer;
  let cookie: string;
  let credential: ProviderCredential;
  let script: Step[] = [];

  beforeEach(async () => {
    server = await createTestServer(kind, { llm: createScriptedLlm(() => script) });
    cookie = await server.signUp('owner@example.com');
    credential = (await (
      await server.request('POST', '/api/providers', { cookie, body: { kind: 'openai', label: 'Work', apiKey: 'sk-test-0123456789abcdef' } })
    ).json()) as ProviderCredential;
  });

  afterEach(async () => {
    await server.close();
  });

  const costs = async () => (await (await server.request('GET', '/api/costs', { cookie })).json()) as CostStatus;
  const newChat = async () => ((await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary).id;
  const messages = async (id: string) =>
    ((await (await server.request('GET', `/api/conversations/${id}`, { cookie })).json()) as ConversationDetail<ChatMessage>).messages;
  const chat = (id: string, text: string) =>
    server.request('POST', `/api/conversations/${id}/chat`, {
      cookie,
      body: { message: { id: `m-${Math.random().toString(36).slice(2)}`, role: 'user', parts: [{ type: 'text', text }] }, credentialId: credential.id, modelId: 'mock-1' },
    });

  it('counts what replies cost and refuses new messages at the limit', async () => {
    expect(await costs()).toMatchObject({ spent: 0, limit: null, own: null, server: null });
    script = [{ text: 'Hallo.' }];
    const id = await newChat();
    await (await chat(id, 'Hallo')).text();
    await eventually(async () => (await messages(id)).length === 2);
    // The reply and the title of the new chat.
    await eventually(async () => Math.abs((await costs()).spent - 2 * CALL) < 1e-9);

    const limited = (await (await server.request('PUT', '/api/costs/limit', { cookie, body: { daily: 0.0001 } })).json()) as CostStatus;
    expect(limited).toMatchObject({ limit: 0.0001, own: 0.0001 });
    const refused = await chat(id, 'Noch was?');
    expect(refused.status).toBe(429);
    expect(((await refused.json()) as { error: { code: string } }).error.code).toBe('cost_limit');
    // Nothing stored for the refused message.
    expect(await messages(id)).toHaveLength(2);

    await server.request('PUT', '/api/costs/limit', { cookie, body: { daily: null } });
    expect((await chat(id, 'Jetzt?')).status).toBe(200);
  });

  it('takes the lower of the admin limit for everyone and the own one', async () => {
    expect((await server.request('PATCH', '/api/admin/settings', { cookie, body: { dailyCostLimit: 0.5 } })).status).toBe(200);
    expect(await costs()).toMatchObject({ limit: 0.5, server: 0.5, own: null });
    await server.request('PUT', '/api/costs/limit', { cookie, body: { daily: 0.2 } });
    expect(await costs()).toMatchObject({ limit: 0.2, server: 0.5, own: 0.2 });
    expect((await server.request('PUT', '/api/costs/limit', { cookie, body: { daily: -1 } })).status).toBe(400);
  });

  it('stops a running reply after the model call that reached the limit', async () => {
    await server.request('PUT', '/api/costs/limit', { cookie, body: { daily: CALL / 2 } });
    // A tool call, then the answer: the second call never comes.
    script = [{ tool: 'calculate', input: { expressions: ['1 + 1'] } }, { text: 'Zwei.' }];
    const id = await newChat();
    const chunks = await readChunks(await chat(id, 'Was ist 1 + 1?'));
    const notice = chunks.find((chunk) => chunk.type === 'data-notice') as { data?: { code?: string } } | undefined;
    expect(notice?.data?.code).toBe('cost_limit');
    expect(chunks.some((chunk) => chunk.type === 'text-delta' && String((chunk as { delta?: unknown }).delta).includes('Zwei'))).toBe(false);
  });

  it('counts running replies and refuses the model calls of devices at the limit', async () => {
    const pairing = (await (await server.request('POST', '/api/pairing', { body: { name: 'PC' }, headers: { origin: '' } })).json()) as {
      userCode: string;
      deviceCode: string;
    };
    await server.request('POST', `/api/devices/pairing/${pairing.userCode}`, { cookie, body: { approve: true } });
    const { token } = (await (
      await server.request('POST', '/api/pairing/token', { body: { deviceCode: pairing.deviceCode }, headers: { origin: '' } })
    ).json()) as { token: string };
    const device = { origin: '', authorization: `Bearer ${token}` };
    const call = () => server.request('POST', `/api/llm/proxy/${credential.id}/chat/completions`, { headers: device, body: { model: 'mock-1', messages: [] } });

    await server.request('PUT', '/api/costs/limit', { cookie, body: { daily: 0.01 } });
    // An agent's reply that has cost 0.02 $ so far counts before it is stored.
    const userId = (await server.database.db.selectFrom('user').select('id').executeTakeFirstOrThrow()).id;
    costGuard(server.database.db).report('c1', userId, 0.02);
    expect((await costs()).spent).toBeCloseTo(0.02, 9);
    const refused = await call();
    expect(refused.status).toBe(429);
    expect(((await refused.json()) as { error: { type: string } }).error.type).toBe('cost_limit');
    // Listing models costs nothing and stays open.
    expect((await server.request('GET', `/api/llm/proxy/${credential.id}/models`, { headers: device })).status).not.toBe(429);
    costGuard(server.database.db).end('c1');
    expect((await costs()).spent).toBe(0);
  });
});
