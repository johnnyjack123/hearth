import type { ConversationSummary, ProviderCredential } from '@harness/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { withReasoning } from '../src/condense.ts';
import { type ChatMessage, saveMessage } from '../src/services/conversations.ts';
import { createScriptedLlm, createTestServer, ORIGIN, readChunks, type TestServer } from './helpers.ts';

const reply = (id: string, modelId: string, credentialId = 'cred-1'): ChatMessage => ({
  id,
  role: 'assistant',
  parts: [
    { type: 'reasoning', text: `gedacht in ${id}` },
    { type: 'text', text: `Antwort ${id}` },
  ],
  metadata: { modelId, credentialId },
});
const ask = (id: string, condensed = false): ChatMessage => ({
  id,
  role: 'user',
  parts: [{ type: 'text', text: id }],
  metadata: condensed ? { condensed: true } : {},
});

describe('reasoning of earlier replies', () => {
  const current = { modelId: 'deepseek/deepseek-v4.1-flash', credentialId: 'cred-1' };
  const reasoningOf = (messages: ChatMessage[]) => messages.map((message) => message.parts.some((part) => part.type === 'reasoning'));

  it('stays when the same model wrote it, so the prompt keeps its start', () => {
    const messages = [ask('u1'), reply('a1', current.modelId), ask('u2'), reply('a2', current.modelId), ask('u3')];
    expect(reasoningOf(withReasoning(messages, current))).toEqual([false, true, false, true, false]);
  });

  it('goes for another model or credential, whose provider may reject it', () => {
    const messages = [ask('u1'), reply('a1', 'anthropic/claude'), ask('u2'), reply('a2', current.modelId, 'cred-2'), ask('u3')];
    const kept = withReasoning(messages, current);
    expect(reasoningOf(kept)).toEqual([false, false, false, false, false]);
    // The answer itself stays.
    expect(kept[1]?.parts).toEqual([{ type: 'text', text: 'Antwort a1' }]);
  });

  it('goes before the last condensing mark, where the chat is long or its cache cold', () => {
    const messages = [ask('u1'), reply('a1', current.modelId), ask('u2', true), reply('a2', current.modelId), ask('u3')];
    expect(reasoningOf(withReasoning(messages, current))).toEqual([false, false, false, true, false]);
  });
});

describe('reasoning in a chat', () => {
  let server: TestServer;
  let llm: ReturnType<typeof createScriptedLlm>;
  let cookie: string;
  let credential: ProviderCredential;

  beforeEach(async () => {
    llm = createScriptedLlm(() => [{ text: 'Ok.' }]);
    server = await createTestServer('sqlite', { llm });
    cookie = await server.signUp('alice@example.com');
    credential = (await (
      await server.request('POST', '/api/providers', { cookie, body: { kind: 'openai', label: 'Work', apiKey: 'sk-test-0123456789abcdef' } })
    ).json()) as ProviderCredential;
  });

  afterEach(async () => {
    await server.close();
  });

  async function send(conversationId: string, modelId: string) {
    const response = await server.app.request(`/api/conversations/${conversationId}/chat`, {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/json' },
      body: JSON.stringify({
        message: { id: `m-${Math.random().toString(36).slice(2)}`, role: 'user', parts: [{ type: 'text', text: 'Weiter' }] },
        credentialId: credential.id,
        modelId,
      }),
    });
    expect(response.status).toBe(200);
    await readChunks(response);
    return JSON.stringify(llm.calls.at(-1)?.prompt);
  }

  it('reaches the same model again, not another one', async () => {
    const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    await saveMessage(server.database.db, id, { id: 'u0', role: 'user', parts: [{ type: 'text', text: 'Frage' }], metadata: { createdAt: 1 } });
    await saveMessage(server.database.db, id, {
      id: 'a0',
      role: 'assistant',
      parts: [
        { type: 'reasoning', text: 'Erst nachdenken.' },
        { type: 'text', text: 'Antwort.' },
      ],
      metadata: { createdAt: 2, modelId: 'mock-1', credentialId: credential.id },
    });
    expect(await send(id, 'mock-1')).toContain('Erst nachdenken.');
    expect(await send(id, 'mock-2')).not.toContain('Erst nachdenken.');
  });
});
