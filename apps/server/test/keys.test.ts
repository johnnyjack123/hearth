import type { ConversationSummary, ProviderCredential } from '@hearth/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createTestServer, DIALECTS, ORIGIN, type TestServer } from './helpers.ts';

describe.each(DIALECTS)('API keys and streamed answers on %s', (kind) => {
  let server: TestServer;
  let cookie: string;

  beforeEach(async () => {
    server = await createTestServer(kind);
    cookie = await server.signUp('owner@example.com');
  });

  afterEach(async () => {
    await server.close();
  });

  it('refuses a key that cannot belong to the provider', async () => {
    const wrong = await server.request('POST', '/api/providers', {
      cookie,
      body: { kind: 'openrouter', label: 'OpenRouter', apiKey: 'JF0aBcDeFgHiJkLmNoPqRsTuVwXyZ012' },
    });
    expect(wrong.status).toBe(400);
    expect(((await wrong.json()) as { error: { message: string } }).error.message).toMatch(/start with sk-or-/);

    const right = await server.request('POST', '/api/providers', {
      cookie,
      body: { kind: 'openrouter', label: 'OpenRouter', apiKey: 'sk-or-v1-0123456789abcdef' },
    });
    expect(right.status).toBe(201);
    const { id } = (await right.json()) as ProviderCredential;
    // Replacing the key is checked the same way.
    expect((await server.request('PATCH', `/api/providers/${id}`, { cookie, body: { apiKey: 'not-a-key' } })).status).toBe(400);
    // Providers whose keys share no prefix take anything.
    const other = await server.request('POST', '/api/providers', {
      cookie,
      body: { kind: 'openai', label: 'OpenAI', apiKey: 'sk-test-0123456789abcdef' },
    });
    expect(other.status).toBe(201);
  });

  it('tells nginx not to buffer a streamed answer', async () => {
    const credential = (await (
      await server.request('POST', '/api/providers', { cookie, body: { kind: 'openai', label: 'Work', apiKey: 'sk-test-0123456789abcdef' } })
    ).json()) as ProviderCredential;
    const { id } = (await (await server.request('POST', '/api/conversations', { cookie, body: {} })).json()) as ConversationSummary;
    const response = await server.app.request(`/api/conversations/${id}/chat`, {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/json' },
      body: JSON.stringify({
        message: { id: 'm-1', role: 'user', parts: [{ type: 'text', text: 'Hallo' }] },
        credentialId: credential.id,
        modelId: 'mock-1',
      }),
    });
    expect(response.headers.get('content-type')).toMatch(/^text\/event-stream/);
    expect(response.headers.get('x-accel-buffering')).toBe('no');
    await response.text();
  });
});
