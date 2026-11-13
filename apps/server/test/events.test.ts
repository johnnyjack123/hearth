import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { CLIENT_ID_HEADER, type ConversationSummary, type LiveEvent, type ProviderCredential } from '@hearth/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { LiveEvents } from '../src/events.ts';
import { createTestServer, eventually, ORIGIN, readChunks, type TestServer } from './helpers.ts';

/** Collects the events of an open `/api/events` response as they arrive. */
function listen(response: Response) {
  const events: LiveEvent[] = [];
  const reader = (response.body as ReadableStream<Uint8Array>).getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  void (async () => {
    for (;;) {
      const { value, done } = await reader.read().catch(() => ({ value: undefined, done: true }));
      if (done || !value) {
        return;
      }
      buffer += decoder.decode(value, { stream: true });
      for (let end = buffer.indexOf('\n\n'); end >= 0; end = buffer.indexOf('\n\n')) {
        const data = buffer
          .slice(0, end)
          .split('\n')
          .find((line) => line.startsWith('data: '));
        buffer = buffer.slice(end + 2);
        if (data) {
          events.push(JSON.parse(data.slice('data: '.length)) as LiveEvent);
        }
      }
    }
  })();
  return { events, stop: () => reader.cancel() };
}

describe('live events', () => {
  let server: TestServer;
  let events: LiveEvents;
  let cookie: string;
  let credential: ProviderCredential;

  beforeEach(async () => {
    events = new LiveEvents();
    server = await createTestServer('sqlite', { events });
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

  const fromPhone = { [CLIENT_ID_HEADER]: 'phone-1' };

  it('tells every open page of the user what changed, with the page that caused it', async () => {
    const laptop = listen(await server.request('GET', '/api/events', { cookie }));
    const otherUser = listen(await server.request('GET', '/api/events', { cookie: await server.signUp('bob@example.com') }));
    await eventually(async () => laptop.events.length > 0 && otherUser.events.length > 0);
    expect(laptop.events[0]).toEqual({ type: 'hello', version: 'dev', commit: null });

    const created = await server.request('POST', '/api/conversations', { cookie, body: {}, headers: fromPhone });
    const { id } = (await created.json()) as ConversationSummary;
    const reply = await server.app.request(`/api/conversations/${id}/chat`, {
      method: 'POST',
      headers: { origin: ORIGIN, cookie, 'content-type': 'application/json', ...fromPhone },
      body: JSON.stringify({
        message: { id: 'm-1', role: 'user', parts: [{ type: 'text', text: 'Hallo vom Handy' }] },
        credentialId: credential.id,
        modelId: 'mock-1',
      }),
    });
    await readChunks(reply);
    await eventually(async () => laptop.events.some((event) => event.type === 'chat' && event.change === 'saved'));
    // The reply is announced once the stream can be resumed, and again once
    // stored. The title comes from the server itself, without a source.
    const fromServer = (event: LiveEvent) => event.type === 'list' && event.source === undefined;
    expect(laptop.events.slice(1).filter((event) => !fromServer(event))).toEqual([
      { type: 'list', source: 'phone-1' },
      { type: 'chat', id, change: 'started', source: 'phone-1' },
      { type: 'chat', id, change: 'saved', source: 'phone-1' },
    ]);
    expect(laptop.events.some(fromServer)).toBe(true);

    // A name that is not one counts as no name.
    await server.request('PATCH', `/api/conversations/${id}`, { cookie, body: { title: 'Neu' }, headers: { [CLIENT_ID_HEADER]: 'no spaces allowed' } });
    await server.request('DELETE', `/api/conversations/${id}`, { cookie, headers: fromPhone });
    await eventually(async () => laptop.events.some((event) => event.type === 'chat' && event.change === 'deleted'));
    expect(laptop.events.slice(-2)).toEqual([{ type: 'list' }, { type: 'chat', id, change: 'deleted', source: 'phone-1' }]);

    // Nothing of it reaches another account.
    expect(otherUser.events).toEqual([{ type: 'hello', version: 'dev', commit: null }]);

    // A closed page stops listening.
    const alice = await userId(server, cookie);
    expect(events.connections(alice)).toBe(1);
    await laptop.stop();
    await eventually(async () => events.connections(alice) === 0);
    await otherUser.stop();
  });

  it('needs a session', async () => {
    expect((await server.request('GET', '/api/events')).status).toBe(401);
  });
});

async function userId(server: TestServer, cookie: string): Promise<string> {
  const session = (await (await server.request('GET', '/api/auth/get-session', { cookie })).json()) as { user: { id: string } };
  return session.user.id;
}

describe('web app files', () => {
  let server: TestServer;

  beforeEach(async () => {
    const dist = mkdtempSync(path.join(tmpdir(), 'hearth-web-'));
    writeFileSync(path.join(dist, 'index.html'), '<!doctype html><html><head><title>Hearth</title></head><body></body></html>');
    writeFileSync(path.join(dist, 'theme-init.js'), 'void 0;');
    mkdirSync(path.join(dist, 'assets'));
    writeFileSync(path.join(dist, 'assets', 'app-abc123.js'), 'void 0;');
    server = await createTestServer('sqlite', { webDist: dist });
  });

  afterEach(async () => {
    await server.close();
  });

  it('keeps no stale page, names its build, and caches only hashed assets', async () => {
    for (const url of ['/', '/index.html', '/c/some-chat']) {
      const page = await server.request('GET', url);
      expect(page.status).toBe(200);
      // An installed app opened days after an update started the old build.
      expect(page.headers.get('cache-control')).toBe('no-cache');
      expect(await page.text()).toContain('<meta name="hearth-build" content="dev" />');
    }
    const theme = await server.request('GET', '/theme-init.js');
    expect(theme.status).toBe(200);
    expect(theme.headers.get('cache-control')).toBe('no-cache');
    const asset = await server.request('GET', '/assets/app-abc123.js');
    expect(asset.status).toBe(200);
    expect(asset.headers.get('cache-control')).toBe('public, max-age=31536000, immutable');
    const old = await server.request('GET', '/assets/app-old999.js');
    expect(old.status).toBe(404);
    expect(old.headers.get('cache-control')).toBeNull();
  });
});
