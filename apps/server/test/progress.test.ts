import type { ConversationDetail, ConversationSummary, ProviderCredential } from '@harness/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { ChatMessage } from '../src/services/conversations.ts';
import { createScriptedLlm, createTestServer, ORIGIN, readChunks, type Step, type TestServer } from './helpers.ts';

describe('progress list in ordinary chats', () => {
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

  it('lets the model show its plan and stores the list with the reply', async () => {
    const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    const items = [
      { text: 'Suchen', state: 'done' },
      { text: 'Lesen', state: 'active' },
      { text: 'Antworten', state: 'pending' },
    ];
    script = [{ tool: 'progress', input: { items } }, { text: 'Fertig.' }];
    await chat(id, 'Recherchiere das gründlich.');
    expect(llm.calls[0]?.tools).toContain('progress');
    expect(llm.calls[0]?.system).toContain('progress tool');
    // The model reads only a short line back.
    expect(JSON.stringify(llm.calls[1]?.prompt)).toContain('Shown: 1/3 done.');

    const detail = (await (await server.request('GET', `/api/conversations/${id}`, { cookie })).json()) as ConversationDetail<ChatMessage>;
    const part = detail.messages[1]?.parts.find((candidate) => candidate.type === 'tool-progress') as
      | { input: unknown; output: unknown }
      | undefined;
    expect(part?.input).toEqual({ items });
    expect(part?.output).toEqual({ done: 1, total: 3 });

    // Switched off, the tool is gone and earlier lists become text.
    await server.request('PATCH', '/api/admin/settings', { cookie, body: { progress: false } });
    script = [{ text: 'Ok.' }];
    await chat(id, 'Danke.');
    expect(llm.calls.at(-1)?.tools).not.toContain('progress');
    expect(JSON.stringify(llm.calls.at(-1)?.prompt)).toContain('[Earlier, progress: Shown: 1/3 done.]');
  });
});
