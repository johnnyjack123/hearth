import type { ConversationSearchResult, ConversationSummary } from '@hearth/shared';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { type ChatMessage, saveMessage } from '../src/services/conversations.ts';
import { fillSearchText, snippet } from '../src/services/search-text.ts';
import { createTestServer, DIALECTS, type TestServer } from './helpers.ts';

describe.each(DIALECTS)('chat search on %s', (kind) => {
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
  const search = async (query: string, as = cookie) =>
    ((await (await server.request('GET', `/api/conversations/search?q=${encodeURIComponent(query)}`, { cookie: as })).json()) as ConversationSearchResult).hits;
  // Distinct times, so "newest first" is well defined.
  let clock = 1_700_000_000_000;
  const say = (conversationId: string, role: 'user' | 'assistant', text: string, extra: ChatMessage['parts'] = []) =>
    saveMessage(server.database.db, conversationId, {
      id: `m-${Math.random().toString(36).slice(2)}`,
      role,
      parts: [{ type: 'text', text }, ...extra],
      metadata: { createdAt: (clock += 1000) },
    } as ChatMessage);

  it('finds chats by the words in their messages, case and accents ignored', async () => {
    const coffee = await newChat();
    await say(coffee, 'user', 'Wie lange sollte man Kaffeebohnen rösten?');
    await say(coffee, 'assistant', 'Helle Röstungen brauchen etwa 9 Minuten, dunkle bis zu 15. Die Straße der Bohnen …');
    const tea = await newChat();
    await say(tea, 'user', 'Grüner Tee: wie heiß darf das Wasser sein?');

    const hits = await search('ROSTUNGEN minuten');
    expect(hits.map((hit) => hit.conversation.id)).toEqual([coffee]);
    const [hit] = hits;
    // The excerpt shows the original text with the places of both words.
    expect(hit?.snippet).toContain('Röstungen');
    expect(hit?.marks.map(([start, end]) => hit.snippet.slice(start, end))).toEqual(['Röstungen', 'Minuten']);
    // ß matches ss; all words have to be in the same message.
    expect((await search('strasse')).map((found) => found.conversation.id)).toEqual([coffee]);
    expect(await search('Röstungen Wasser')).toEqual([]);
    // Several matching messages in one chat count, the newest match comes first.
    expect((await search('wie')).map((found) => [found.conversation.id, found.matches])).toEqual([
      [tea, 1],
      [coffee, 1],
    ]);
  });

  it('looks only at visible text, and only in the own chats', async () => {
    const chat = await newChat();
    await say(chat, 'assistant', 'Hier ist die Antwort.', [
      { type: 'reasoning', text: 'geheimer Gedankengang' },
      { type: 'dynamic-tool', toolName: 'bash', toolCallId: 't1', state: 'output-available', input: { command: 'ls' }, output: 'werkzeugausgabe' },
    ] as ChatMessage['parts']);
    expect(await search('Gedankengang')).toEqual([]);
    expect(await search('werkzeugausgabe')).toEqual([]);
    expect(await search('Antwort')).toHaveLength(1);
    // Wildcards are searched for literally; one letter alone finds nothing.
    expect(await search('%')).toEqual([]);
    expect(await search('a')).toEqual([]);

    const other = await server.signUp('other@example.com');
    expect(await search('Antwort', other)).toEqual([]);
    expect((await server.request('GET', '/api/conversations/search?q=Antwort')).status).toBe(401);
  });

  it('fills the search text of messages stored before the search existed', async () => {
    const chat = await newChat();
    await say(chat, 'user', 'Ein alter Eintrag über Sauerteig');
    await server.database.db.updateTable('message').set({ searchText: null }).execute();
    expect(await search('Sauerteig')).toEqual([]);
    await fillSearchText(server.database.db);
    expect(await search('sauerteig')).toHaveLength(1);
  });
});

describe('search excerpts', () => {
  it('cuts long texts at spaces around the first match and keeps the marks in place', () => {
    const text = `${'Vorrede '.repeat(20)}Hier steht das gesuchte Wort mitten drin.${' Nachrede'.repeat(30)}`;
    const { text: excerpt, marks } = snippet(text, ['gesuchte']);
    expect(excerpt.startsWith('… ')).toBe(true);
    expect(excerpt.endsWith(' …')).toBe(true);
    expect(marks.map(([start, end]) => excerpt.slice(start, end))).toEqual(['gesuchte']);
    expect(excerpt).not.toMatch(/^… \S*orrede\S/);
  });
});
