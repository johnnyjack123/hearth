import type { ConversationDetail, ConversationSummary, ProviderCredential, Tone } from '@harness/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { chatNote } from '../src/routes/chat.ts';
import { agentToneNote } from '../src/routes/conversations.ts';
import type { ChatMessage } from '../src/services/conversations.ts';
import { createScriptedLlm, createTestServer, ORIGIN, readChunks, type Step, type TestServer } from './helpers.ts';

function sent(tone?: Tone, date = '2026-11-08'): ChatMessage {
  return { id: 'u', role: 'user', parts: [{ type: 'text', text: 'x' }], metadata: { date, ...(tone ? { tone } : {}) } };
}

describe('tone notes', () => {
  const today = { date: '2026-11-08', research: null, verbosity: 'medium' as const };

  it('name a tone other than neutral with every message, and neutral once after it', () => {
    expect(chatNote([], { ...today, tone: 'neutral' })).toBe('[LLM Harness: Today is 2026-11-08.]');
    expect(chatNote([], { ...today, tone: 'scientific' })).toBe('[LLM Harness: Today is 2026-11-08. Tone: scientific.]');
    expect(chatNote([sent('scientific')], { ...today, tone: 'scientific' })).toBe('[LLM Harness: Tone: scientific.]');
    expect(chatNote([sent('formal')], { ...today, tone: 'neutral' })).toBe('[LLM Harness: Tone: neutral.]');
    expect(chatNote([sent()], { ...today, tone: 'neutral' })).toBeNull();
    // Without a tone it is neutral, as for older callers.
    expect(chatNote([sent()], today)).toBeNull();
  });

  it('tell an agent in words it understands without the chat prompt', () => {
    expect(agentToneNote([], 'neutral')).toBeNull();
    expect(agentToneNote([], 'scientific')).toMatch(/Never invent facts/);
    expect(agentToneNote([], 'scientific')).toMatch(/label it explicitly as your guess/);
    expect(agentToneNote([sent('critical')], 'critical')).toMatch(/critical/);
    expect(agentToneNote([sent('relaxed')], 'neutral')).toBe('[LLM Harness: Write in your usual tone again.]');
  });
});

describe('tone', () => {
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

  async function chat(conversationId: string, text: string, tone?: string) {
    const response = await server.app.request(`/api/conversations/${conversationId}/chat`, {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/json' },
      body: JSON.stringify({
        message: { id: `m-${Math.random().toString(36).slice(2)}`, role: 'user', parts: [{ type: 'text', text }] },
        credentialId: credential.id,
        modelId: 'mock-1',
        ...(tone ? { tone } : {}),
      }),
    });
    return response;
  }

  it('reaches the model as a note, so the system prompt stays the same for the cache', async () => {
    const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    await readChunks(await chat(id, 'Wie alt ist das Universum?', 'scientific'));
    const first = llm.calls.at(-1);
    // The system prompt explains every tone, including what scientific asks for;
    // this server has no search, so research mode stays unmentioned.
    expect(first?.system).toContain('"scientific"');
    expect(first?.system).toContain('label it explicitly as your own guess');
    expect(first?.system).not.toContain('research mode');
    expect(JSON.stringify(first?.prompt.at(-1))).toContain('Tone: scientific.');
    await readChunks(await chat(id, 'Und die Erde?'));
    const second = llm.calls.at(-1);
    expect(second?.system).toBe(first?.system);
    expect(JSON.stringify(second?.prompt.at(-1))).toContain('Tone: neutral.');

    const detail = (await (await server.request('GET', `/api/conversations/${id}`, { cookie })).json()) as ConversationDetail<ChatMessage>;
    expect(detail.messages.filter((message) => message.role === 'user').map((message) => message.metadata?.tone)).toEqual(['scientific', undefined]);
  });

  it('is refused when unknown', async () => {
    const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    expect((await chat(id, 'x', 'sarcastic')).status).toBe(400);
  });
});
