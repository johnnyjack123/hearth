import type { ConversationDetail, ConversationSummary, ProviderCredential, Verbosity } from '@harness/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { chatNote } from '../src/routes/chat.ts';
import { agentVerbosityNote } from '../src/routes/conversations.ts';
import type { ChatMessage } from '../src/services/conversations.ts';
import { createScriptedLlm, createTestServer, ORIGIN, readChunks, type Step, type TestServer } from './helpers.ts';

function sent(verbosity?: Verbosity, date = '2026-11-06'): ChatMessage {
  return { id: 'u', role: 'user', parts: [{ type: 'text', text: 'x' }], metadata: { date, ...(verbosity ? { verbosity } : {}) } };
}

describe('answer length notes', () => {
  const today = { date: '2026-11-06', research: null };

  it('name a length other than medium with every message, and medium once after it', () => {
    expect(chatNote([], { ...today, verbosity: 'medium' })).toBe('[LLM Harness: Today is 2026-11-06.]');
    expect(chatNote([], { ...today, verbosity: 'short' })).toBe('[LLM Harness: Today is 2026-11-06. Answer length: short.]');
    // Repeated while it holds: models drift back when the setting lies far behind.
    expect(chatNote([sent('short')], { ...today, verbosity: 'short' })).toBe('[LLM Harness: Answer length: short.]');
    expect(chatNote([sent('short')], { ...today, verbosity: 'medium' })).toBe('[LLM Harness: Answer length: medium.]');
    expect(chatNote([sent()], { ...today, verbosity: 'medium' })).toBeNull();
    expect(chatNote([sent()], { ...today, verbosity: 'detailed' })).toBe('[LLM Harness: Answer length: detailed.]');
  });

  it('tell an agent the same way, in words it understands without the chat prompt', () => {
    expect(agentVerbosityNote([], 'medium')).toBeNull();
    expect(agentVerbosityNote([], 'short')).toMatch(/^\[LLM Harness: Keep your answers short/);
    expect(agentVerbosityNote([sent('short')], 'short')).toMatch(/short/);
    expect(agentVerbosityNote([sent('detailed')], 'medium')).toBe('[LLM Harness: Answer at your usual length again.]');
    expect(agentVerbosityNote([sent()], 'medium')).toBeNull();
  });
});

describe('answer length', () => {
  let server: TestServer;
  let llm: ReturnType<typeof createScriptedLlm>;
  const script: Step[] = [{ text: 'Ok.' }];
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

  async function chat(conversationId: string, text: string, verbosity?: Verbosity) {
    const response = await server.app.request(`/api/conversations/${conversationId}/chat`, {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/json' },
      body: JSON.stringify({
        message: { id: `m-${Math.random().toString(36).slice(2)}`, role: 'user', parts: [{ type: 'text', text }] },
        credentialId: credential.id,
        modelId: 'mock-1',
        ...(verbosity ? { verbosity } : {}),
      }),
    });
    expect(response.status).toBe(200);
    return readChunks(response);
  }

  it('reaches the model as a note, so the system prompt stays the same for the cache', async () => {
    const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    await chat(id, 'Erste Frage', 'short');
    const first = llm.calls.at(-1);
    expect(first?.system).toContain('Answer length:');
    expect(JSON.stringify(first?.prompt.at(-1))).toContain('Answer length: short.');
    await chat(id, 'Zweite Frage');
    const second = llm.calls.at(-1);
    expect(second?.system).toBe(first?.system);
    expect(JSON.stringify(second?.prompt.at(-1))).toContain('Answer length: medium.');
    await chat(id, 'Dritte Frage');
    expect(JSON.stringify(llm.calls.at(-1)?.prompt.at(-1))).not.toContain('Answer length');

    // The chat opens with the length it was last used with.
    const detail = (await (await server.request('GET', `/api/conversations/${id}`, { cookie })).json()) as ConversationDetail<ChatMessage>;
    expect(detail.messages.filter((message) => message.role === 'user').map((message) => message.metadata?.verbosity)).toEqual([
      'short',
      undefined,
      undefined,
    ]);
  });

  it('is refused when unknown', async () => {
    const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    const response = await server.app.request(`/api/conversations/${id}/chat`, {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/json' },
      body: JSON.stringify({
        message: { id: 'm-1', role: 'user', parts: [{ type: 'text', text: 'x' }] },
        credentialId: credential.id,
        modelId: 'mock-1',
        verbosity: 'endless',
      }),
    });
    expect(response.status).toBe(400);
  });
});
