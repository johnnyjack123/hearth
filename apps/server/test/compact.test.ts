import type { ConversationDetail, ConversationSummary, ProviderCredential } from '@hearth/shared';
import { simulateReadableStream } from 'ai';
import { MockLanguageModelV4 } from 'ai/test';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { compactionRange, latestCompaction, shouldCompact, withCompaction } from '../src/compact.ts';
import { effortOptions } from '../src/effort.ts';
import type { ChatMessage } from '../src/services/conversations.ts';
import { createMockLlm, createTestServer, eventually, type MockLlm, ORIGIN, type TestServer } from './helpers.ts';

/** A message whose parts are about `tokens` tokens long (four characters each). */
function message(id: string, role: 'user' | 'assistant', tokens = 10): ChatMessage {
  return { id, role, parts: [{ type: 'text', text: 'x'.repeat(Math.max(0, tokens * 4 - 30)) }] };
}

function summaryPart(through: string, summary = 'Kurz gesagt'): ChatMessage['parts'][number] {
  return { type: 'data-compaction', id: `compaction-${through}`, data: { state: 'done', trigger: 'auto', through, summary } };
}

describe('compaction rules', () => {
  const chat = [
    message('u1', 'user'),
    message('a1', 'assistant', 1_000),
    message('u2', 'user'),
    message('a2', 'assistant', 1_000),
    message('u3', 'user'),
    message('a3', 'assistant', 100),
  ];

  it('keeps the latest turns that fit and never splits a turn', () => {
    // Asked for: everything but the latest turn.
    expect(compactionRange(chat, 0, true)).toEqual({ from: 0, to: 4 });
    // Automatic: the latest turns while they fit, the first one always goes.
    expect(compactionRange(chat, 200, false)).toEqual({ from: 0, to: 4 });
    expect(compactionRange(chat, 5_000, false)).toEqual({ from: 0, to: 2 });
    expect(compactionRange(chat, 0, false)).toEqual({ from: 0, to: 6 });
    // One turn: nothing to summarize when asked, all of it automatically.
    expect(compactionRange(chat.slice(0, 2), 0, true)).toBeNull();
    expect(compactionRange(chat.slice(0, 2), 0, false)).toEqual({ from: 0, to: 2 });
  });

  it('continues after the latest summary and shows the model only what follows', () => {
    const summarized = [...chat.slice(0, 3), { ...chat[3], parts: [...(chat[3] as ChatMessage).parts, summaryPart('a1')] } as ChatMessage, ...chat.slice(4)];
    expect(latestCompaction(summarized)).toMatchObject({ through: 'a1', index: 1 });
    expect(compactionRange(summarized, 0, true)).toEqual({ from: 2, to: 4 });

    const seen = withCompaction(summarized);
    expect(seen.map((m) => m.id)).toEqual(['u2', 'a2', 'u3', 'a3']);
    const first = seen[0]?.parts[0];
    expect(first).toMatchObject({ type: 'text' });
    expect((first as { text: string }).text).toMatch(/^\[Hearth: Summary of the earlier part of this chat.*\]\n\nKurz gesagt\n\n\[End of the summary\.\]$/s);
    // A summary whose last message is gone no longer counts.
    expect(latestCompaction(summarized.filter((m) => m.id !== 'a1'))).toBeNull();
    expect(withCompaction(chat)).toBe(chat);
  });

  it('summarizes automatically only when the context runs full', () => {
    const settings = { enabled: true, reserveTokens: 16_384, keepRecentTokens: 20_000 };
    const withUsage = (contextTokens: number) => [
      message('u1', 'user'),
      { ...message('a1', 'assistant'), metadata: { usage: { contextTokens } } },
    ];
    const next = message('u2', 'user');
    expect(shouldCompact(withUsage(100_000), next, settings, undefined)).toBe(false);
    expect(shouldCompact(withUsage(100_000), next, settings, 128_000)).toBe(false);
    expect(shouldCompact(withUsage(112_000), next, settings, 128_000)).toBe(true);
    expect(shouldCompact(withUsage(112_000), next, { ...settings, enabled: false }, 128_000)).toBe(false);
    // A small window keeps a quarter free at most, not more than it has.
    expect(shouldCompact(withUsage(5_000), next, settings, 8_000)).toBe(false);
    expect(shouldCompact(withUsage(6_500), next, settings, 8_000)).toBe(true);
  });
});

describe('effort per provider', () => {
  it('uses the portable level, OpenRouter its reasoning object and DeepSeek its own switch', () => {
    expect(effortOptions('anthropic', null, undefined)).toEqual({});
    expect(effortOptions('anthropic', null, 'high')).toEqual({ reasoning: 'high' });
    expect(effortOptions('google', null, 'off')).toEqual({ reasoning: 'none' });
    expect(effortOptions('openrouter', null, 'xhigh')).toEqual({ providerOptions: { openrouter: { reasoning: { effort: 'xhigh' } } } });
    expect(effortOptions('openrouter', null, 'off')).toEqual({ providerOptions: { openrouter: { reasoning: { effort: 'none' } } } });
    expect(effortOptions('openai-compatible', 'https://api.deepseek.com/v1', 'off')).toEqual({
      providerOptions: { 'openai-compatible': { thinking: { type: 'disabled' } } },
    });
    expect(effortOptions('openai-compatible', 'https://api.deepseek.com/v1', 'max')).toEqual({ reasoning: 'max' });
    expect(effortOptions('openai-compatible', 'http://localhost:1234/v1', 'off')).toEqual({ reasoning: 'none' });
  });
});

type Prompt = { role: string; content: unknown }[];

/** A model that records what each streamed call got and writes summaries on request. */
function createRecordingLlm(): MockLlm & {
  streams: { prompt: Prompt; reasoning: unknown; providerOptions: unknown }[];
  summaries: string[];
  nextInput: { tokens: number };
  failSummary: { now: boolean };
} {
  const base = createMockLlm();
  const streams: { prompt: Prompt; reasoning: unknown; providerOptions: unknown }[] = [];
  const summaries: string[] = [];
  const nextInput = { tokens: 5 };
  const failSummary = { now: false };
  const usage = (input: number) => ({
    inputTokens: { total: input, noCache: input, cacheRead: undefined, cacheWrite: undefined },
    outputTokens: { total: 7, text: 7, reasoning: undefined },
  });
  return Object.assign(base, {
    streams,
    summaries,
    nextInput,
    failSummary,
    createModel(_credential: unknown, modelId: string) {
      base.createdModels.push(modelId);
      return new MockLanguageModelV4({
        doGenerate: async (options) => {
          const prompt = JSON.stringify(options.prompt);
          if (prompt.includes('You summarize the earlier part of a chat')) {
            summaries.push(prompt);
            if (failSummary.now) {
              throw new Error('Zusammenfassung kaputt');
            }
            return {
              content: [{ type: 'text', text: 'Bisher: Kaffee wird bei 200 °C geröstet.' }],
              finishReason: { unified: 'stop', raw: undefined },
              usage: usage(400),
              warnings: [],
            };
          }
          return { content: [{ type: 'text', text: 'Titel' }], finishReason: { unified: 'stop', raw: undefined }, usage: usage(5), warnings: [] };
        },
        doStream: async (options) => {
          streams.push({ prompt: options.prompt as Prompt, reasoning: options.reasoning, providerOptions: options.providerOptions });
          return {
            stream: simulateReadableStream({
              chunks: [
                { type: 'text-start', id: 't1' },
                { type: 'text-delta', id: 't1', delta: `Antwort ${streams.length}` },
                { type: 'text-end', id: 't1' },
                { type: 'finish', finishReason: { unified: 'stop', raw: undefined }, usage: usage(nextInput.tokens) },
              ] as never[],
            }),
          };
        },
      });
    },
  });
}

/** Reads a streamed reply to its end and returns the UI message chunks. */
async function readChunks(response: Response) {
  const chunks: { type: string; [key: string]: unknown }[] = [];
  const text = await response.text();
  for (const line of text.split('\n')) {
    if (line.startsWith('data: ') && line !== 'data: [DONE]') {
      chunks.push(JSON.parse(line.slice('data: '.length)) as { type: string });
    }
  }
  return chunks;
}

describe('effort and summaries in chats', () => {
  let server: TestServer;
  let llm: ReturnType<typeof createRecordingLlm>;
  let cookie: string;

  beforeEach(async () => {
    llm = createRecordingLlm();
    server = await createTestServer('sqlite', { llm });
    cookie = await server.signUp('alice@example.com');
  });

  afterEach(async () => {
    await server.close();
  });

  async function credential(kind: string, baseUrl?: string): Promise<ProviderCredential> {
    const response = await server.request('POST', '/api/providers', {
      cookie,
      body: { kind, label: kind, apiKey: 'sk-test-0123456789abcdef', ...(baseUrl ? { baseUrl } : {}) },
    });
    return (await response.json()) as ProviderCredential;
  }

  async function newConversation(): Promise<string> {
    const created = await server.request('POST', '/api/conversations', { cookie, body: {} });
    return ((await created.json()) as ConversationSummary).id;
  }

  function chat(conversationId: string, credentialId: string, text: string, extra: Record<string, unknown> = {}) {
    return server.app.request(`/api/conversations/${conversationId}/chat`, {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/json' },
      body: JSON.stringify({
        message: { id: `m-${Math.random().toString(36).slice(2)}`, role: 'user', parts: [{ type: 'text', text }] },
        credentialId,
        modelId: 'mock-1',
        ...extra,
      }),
    });
  }

  async function load(conversationId: string): Promise<ConversationDetail<ChatMessage>> {
    const response = await server.request('GET', `/api/conversations/${conversationId}`, { cookie });
    return (await response.json()) as ConversationDetail<ChatMessage>;
  }

  it('passes the effort on, stores it with the message and keeps the prompt the same', async () => {
    const deepseek = await credential('openai-compatible', 'https://api.deepseek.com/v1');
    const conversationId = await newConversation();
    await readChunks(await chat(conversationId, deepseek.id, 'Erste Frage', { effort: 'high' }));
    await eventually(async () => (await load(conversationId)).messages.length === 2);
    await readChunks(await chat(conversationId, deepseek.id, 'Zweite Frage', { effort: 'off' }));
    await eventually(async () => (await load(conversationId)).messages.length === 4);
    await readChunks(await chat(conversationId, deepseek.id, 'Dritte Frage'));

    expect(llm.streams[0]?.reasoning).toBe('high');
    expect(llm.streams[1]?.reasoning).toBeUndefined();
    expect(llm.streams[1]?.providerOptions).toMatchObject({ 'openai-compatible': { thinking: { type: 'disabled' } } });
    // Without a level the provider decides, as before.
    expect(llm.streams[2]?.reasoning).toBeUndefined();
    expect(llm.streams[2]?.providerOptions).toBeUndefined();
    // The effort is no part of the prompt, so the provider's cache still fits.
    expect(JSON.stringify(llm.streams[1]?.prompt.slice(0, 2))).toBe(JSON.stringify(llm.streams[0]?.prompt.slice(0, 2)));

    await eventually(async () => (await load(conversationId)).messages.length === 6);
    const users = (await load(conversationId)).messages.filter((m) => m.role === 'user');
    expect(users.map((m) => m.metadata?.effort)).toEqual(['high', 'off', undefined]);
  });

  it('summarizes once the context runs full, at a fixed place', async () => {
    // The test catalog gives mock-1 of OpenAI a window of 1,000 tokens.
    const openai = await credential('openai');
    const conversationId = await newConversation();
    llm.nextInput.tokens = 900;
    await readChunks(await chat(conversationId, openai.id, 'Wie röstet man Kaffee?'));
    await eventually(async () => (await load(conversationId)).messages.length === 2);
    const firstReply = (await load(conversationId)).messages[1] as ChatMessage;

    llm.nextInput.tokens = 50;
    const chunks = await readChunks(await chat(conversationId, openai.id, 'Und wie lange?'));
    const states = chunks.filter((chunk) => chunk.type === 'data-compaction').map((chunk) => (chunk.data as { state: string }).state);
    expect(states).toEqual(['running', 'done']);
    expect(chunks[0]).toMatchObject({ type: 'start' });
    expect(llm.summaries).toHaveLength(1);
    expect(llm.summaries[0]).toContain('Wie röstet man Kaffee?');
    // The model reads the summary instead of the first turn.
    const seen = JSON.stringify(llm.streams[1]?.prompt);
    expect(seen).toContain('Bisher: Kaffee wird bei 200 °C geröstet.');
    expect(seen).not.toContain('Wie röstet man Kaffee?');

    await eventually(async () => (await load(conversationId)).messages.length === 4);
    const reply = (await load(conversationId)).messages[3] as ChatMessage;
    expect(reply.parts[0]).toMatchObject({ type: 'data-compaction', data: { state: 'done', trigger: 'auto', through: firstReply.id } });
    expect(reply.metadata?.usage?.helpers?.find((call) => call.purpose === 'compact')).toMatchObject({ model: 'mock-1', input: 400 });

    // The next request starts exactly the same way and summarizes nothing new.
    await readChunks(await chat(conversationId, openai.id, 'Danke'));
    expect(llm.summaries).toHaveLength(1);
    expect(JSON.stringify(llm.streams[2]?.prompt.slice(0, 2))).toBe(JSON.stringify(llm.streams[1]?.prompt.slice(0, 2)));
  });

  it('goes on without a summary that fails', async () => {
    const openai = await credential('openai');
    const conversationId = await newConversation();
    llm.nextInput.tokens = 900;
    await readChunks(await chat(conversationId, openai.id, 'Erste Frage'));
    await eventually(async () => (await load(conversationId)).messages.length === 2);
    llm.failSummary.now = true;
    const chunks = await readChunks(await chat(conversationId, openai.id, 'Zweite Frage'));
    expect(chunks.filter((chunk) => chunk.type === 'data-compaction').at(-1)).toMatchObject({ data: { state: 'failed' } });
    expect(chunks.some((chunk) => chunk.type === 'text-delta')).toBe(true);
    expect(JSON.stringify(llm.streams[1]?.prompt)).toContain('Erste Frage');
  });

  it('summarizes on request, all but the latest turn', async () => {
    const openai = await credential('openrouter');
    const conversationId = await newConversation();
    const compact = () =>
      server.request('POST', `/api/conversations/${conversationId}/compact`, { cookie, body: { credentialId: openai.id, modelId: 'mock-1' } });
    await readChunks(await chat(conversationId, openai.id, 'Erste Frage'));
    await eventually(async () => (await load(conversationId)).messages.length === 2);
    expect((await compact()).status).toBe(409);

    await readChunks(await chat(conversationId, openai.id, 'Zweite Frage'));
    await eventually(async () => (await load(conversationId)).messages.length === 4);
    const before = (await load(conversationId)).messages;
    const response = await compact();
    expect(response.status).toBe(200);
    const updated = (await response.json()) as ChatMessage;
    expect(updated.id).toBe(before[3]?.id);
    expect(updated.parts.at(-1)).toMatchObject({ type: 'data-compaction', data: { state: 'done', trigger: 'manual', through: before[1]?.id } });
    expect(updated.metadata?.usage?.helpers?.find((call) => call.purpose === 'compact')).toBeDefined();
    // Stored, and the model reads the latest turn and the summary only.
    expect((await load(conversationId)).messages[3]).toEqual(updated);
    await readChunks(await chat(conversationId, openai.id, 'Dritte Frage'));
    const seen = JSON.stringify(llm.streams.at(-1)?.prompt);
    expect(seen).not.toContain('Erste Frage');
    expect(seen).toContain('Zweite Frage');
    expect(seen).toContain('Bisher: Kaffee');

    // Nobody else's chat.
    const mallory = await server.signUp('mallory@example.com');
    const foreign = await server.request('POST', `/api/conversations/${conversationId}/compact`, {
      cookie: mallory,
      body: { credentialId: openai.id, modelId: 'mock-1' },
    });
    expect(foreign.status).toBe(404);
  });
});
