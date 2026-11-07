import type { ConversationSummary } from '@harness/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createTestServer, DIALECTS, type TestServer } from './helpers.ts';

describe.each(DIALECTS)('chat tags on %s', (kind) => {
  let server: TestServer;
  let cookie: string;

  beforeEach(async () => {
    server = await createTestServer(kind);
    cookie = await server.signUp('owner@example.com');
  });

  afterEach(async () => {
    await server.close();
  });

  const newChat = async (as = cookie) => ((await (await server.request('POST', '/api/conversations', { cookie: as, body: {} })).json()) as ConversationSummary).id;
  const list = async (as = cookie) => (await (await server.request('GET', '/api/conversations', { cookie: as })).json()) as ConversationSummary[];
  const tagsOf = async (id: string) => (await list()).find((chat) => chat.id === id)?.tags;
  const tag = (id: string, tags: string[], as = cookie) => server.request('PUT', `/api/conversations/${id}/tags`, { cookie: as, body: { tags } });

  it('files chats under tags, spelled like the tag that is already there', async () => {
    const first = await newChat();
    const second = await newChat();
    expect(await tagsOf(first)).toEqual([]);

    const tagged = await tag(first, ['Arbeit', ' Urlaub ', 'arbeit']);
    expect(tagged.status).toBe(200);
    expect(((await tagged.json()) as ConversationSummary).tags).toEqual(['Arbeit', 'Urlaub']);
    // Another chat joins the folder whatever the case.
    await tag(second, ['ARBEIT']);
    expect(await tagsOf(second)).toEqual(['Arbeit']);
    // Tags already on a chat keep their place when one is added.
    await tag(first, ['Neu', 'Urlaub', 'Arbeit']);
    expect(await tagsOf(first)).toEqual(['Arbeit', 'Urlaub', 'Neu']);
    // The chat itself shows them as well.
    const detail = (await (await server.request('GET', `/api/conversations/${first}`, { cookie })).json()) as ConversationSummary;
    expect(detail.tags).toEqual(['Arbeit', 'Urlaub', 'Neu']);

    await tag(first, []);
    expect(await tagsOf(first)).toEqual([]);
  });

  it('rejects names it cannot show and too many tags', async () => {
    const id = await newChat();
    expect((await tag(id, [''])).status).toBe(400);
    expect((await tag(id, ['x'.repeat(41)])).status).toBe(400);
    expect((await tag(id, ['ab\u{202E}cd'])).status).toBe(400);
    expect((await tag(id, Array.from({ length: 11 }, (_, index) => `t${index}`))).status).toBe(400);
  });

  it('renames and removes a tag on every chat, and merges into a tag in use', async () => {
    const first = await newChat();
    const second = await newChat();
    await tag(first, ['Arbeit', 'Projekt']);
    await tag(second, ['Projekt']);

    const renamed = await server.request('PATCH', '/api/tags', { cookie, body: { from: 'projekt', to: 'Kunde A' } });
    expect(await renamed.json()).toEqual({ chats: 2 });
    expect(await tagsOf(first)).toEqual(['Arbeit', 'Kunde A']);
    expect(await tagsOf(second)).toEqual(['Kunde A']);

    // Renaming to a tag in use merges both; a chat with both keeps one.
    await server.request('PATCH', '/api/tags', { cookie, body: { from: 'Kunde A', to: 'arbeit' } });
    expect(await tagsOf(first)).toEqual(['Arbeit']);
    expect(await tagsOf(second)).toEqual(['Arbeit']);

    const removed = await server.request('DELETE', '/api/tags', { cookie, body: { name: 'Arbeit' } });
    expect(await removed.json()).toEqual({ chats: 2 });
    expect((await list()).map((chat) => chat.tags)).toEqual([[], []]);
    // The chats stay.
    expect(await list()).toHaveLength(2);
    expect((await server.request('DELETE', '/api/tags', { cookie, body: { name: 'Arbeit' } })).status).toBe(404);
  });

  it('keeps tags to their owner', async () => {
    const mine = await newChat();
    await tag(mine, ['Privat']);
    const other = await server.signUp('other@example.com');
    expect((await tag(mine, ['Fremd'], other)).status).toBe(404);
    expect((await server.request('PATCH', '/api/tags', { cookie: other, body: { from: 'Privat', to: 'Weg' } })).status).toBe(404);
    expect((await server.request('DELETE', '/api/tags', { cookie: other, body: { name: 'Privat' } })).status).toBe(404);
    expect(await tagsOf(mine)).toEqual(['Privat']);
  });

  it('lists tagged chats beyond the latest ones, so their folders are complete', async () => {
    const old = await newChat();
    await tag(old, ['Archiv']);
    await server.database.db.updateTable('conversation').set({ updatedAt: 1 }).where('id', '=', old).execute();
    const plain = await newChat();
    await server.database.db.updateTable('conversation').set({ updatedAt: 2 }).where('id', '=', plain).execute();
    for (let index = 0; index < 3; index += 1) {
      await newChat();
    }
    const { listConversations } = await import('../src/services/conversations.ts');
    const user = await server.database.db.selectFrom('user').select('id').where('email', '=', 'owner@example.com').executeTakeFirstOrThrow();
    const listed = await listConversations(server.database.db, user.id, 3);
    expect(listed).toHaveLength(4);
    expect(listed.at(-1)).toMatchObject({ id: old, tags: ['Archiv'] });
    expect(listed.some((chat) => chat.id === plain)).toBe(false);
  });
});
