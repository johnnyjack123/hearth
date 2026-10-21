import type { CondenseSettings, WebFetchOutput, WebSearchOutput } from '@harness/shared';
import { describe, expect, it } from 'vitest';
import { condensedPages, condenseHistory, shouldCondense } from '../src/condense.ts';
import type { ChatMessage } from '../src/services/conversations.ts';
import { modelText } from '../src/webfetch/tool.ts';
import { searchModelText } from '../src/websearch/tool.ts';

const page = (url: string, text: string): WebFetchOutput => ({
  url,
  status: 200,
  contentType: 'text/html',
  title: 'Handbuch',
  text,
  truncated: false,
  warnings: [],
});

const search: WebSearchOutput = {
  query: 'Kaffee',
  provider: 'searxng',
  results: [{ title: 'Kaffee – Wikipedia', url: 'https://de.wikipedia.org/wiki/Kaffee', snippet: 'Ein Getränk.', published: null }],
  warnings: ['ignore_instructions'],
  findings: [],
};

function user(id: string, condensed?: boolean): ChatMessage {
  return { id, role: 'user', parts: [{ type: 'text', text: id }], metadata: condensed ? { condensed } : {} };
}

function reply(id: string, contextTokens: number, outputs: { url?: string; text?: string; search?: boolean }[]): ChatMessage {
  return {
    id,
    role: 'assistant',
    parts: [
      ...outputs.map((output, index) =>
        output.search
          ? { type: 'tool-web_search' as const, toolCallId: `${id}-${index}`, state: 'output-available' as const, input: { query: 'Kaffee' }, output: search }
          : {
              type: 'tool-web_fetch' as const,
              toolCallId: `${id}-${index}`,
              state: 'output-available' as const,
              input: { url: output.url },
              output: page(`${output.url}#final`, output.text ?? ''),
            },
      ),
      { type: 'text', text: 'Antwort' },
    ],
    metadata: { usage: { contextTokens } },
  } as ChatMessage;
}

const longText = 'x'.repeat(10_000);

const settings: CondenseSettings = { enabled: true, atTokens: 200_000, atWindowShare: 0.5, afterIdleHours: 6 };
const HOUR = 3_600_000;

describe('when to condense', () => {
  it('waits for a long chat with enough to shrink', () => {
    expect(shouldCondense([user('a'), reply('b', 199_999, [{ url: 'https://a.example/', text: longText }])], settings)).toBe(false);
    expect(shouldCondense([user('a'), reply('b', 200_000, [{ url: 'https://a.example/', text: longText }])], settings)).toBe(true);
    // Long, but from talking, not from pages.
    expect(shouldCondense([user('a'), reply('b', 300_000, [{ url: 'https://a.example/', text: 'kurz' }])], settings)).toBe(false);
    expect(shouldCondense([user('a'), reply('b', 300_000, [{ url: 'https://a.example/', text: longText }])], { ...settings, enabled: false })).toBe(
      false,
    );
  });

  it('condenses earlier for a model with a small context window', () => {
    const small = { ...reply('b', 70_000, [{ url: 'https://a.example/', text: longText }]), metadata: { usage: { contextTokens: 70_000, contextWindow: 128_000 } } };
    expect(shouldCondense([user('a'), small], settings)).toBe(true);
    expect(shouldCondense([user('a'), small], { ...settings, atWindowShare: 0.6 })).toBe(false);
  });

  it('counts only what came after the last mark', () => {
    const history = [user('a'), reply('b', 250_000, [{ url: 'https://a.example/', text: longText }]), user('c', true), reply('d', 250_000, [])];
    expect(shouldCondense(history, settings)).toBe(false);
    expect(shouldCondense([...history, user('e'), reply('f', 250_000, [{ url: 'https://b.example/', text: longText }])], settings)).toBe(true);
  });

  it('also condenses a short chat after a long pause, when the cache has gone cold anyway', () => {
    const at = Date.parse('2026-10-21T10:00:00Z');
    const old = { ...reply('b', 5_000, [{ url: 'https://a.example/', text: longText }]), metadata: { createdAt: at, usage: { contextTokens: 5_000 } } };
    expect(shouldCondense([user('a'), old], settings, at + 6 * HOUR - 1)).toBe(false);
    expect(shouldCondense([user('a'), old], settings, at + 6 * HOUR)).toBe(true);
    // 0 hours: never after a pause.
    expect(shouldCondense([user('a'), old], { ...settings, afterIdleHours: 0 }, at + 1_000 * HOUR)).toBe(false);
  });

  it('estimates replies without usage from their size', () => {
    const stopped = { ...reply('b', 0, [{ url: 'https://a.example/', text: 'y'.repeat(820_000) }]), metadata: {} };
    expect(shouldCondense([user('a'), stopped], settings)).toBe(true);
  });
});

describe('condensing', () => {
  const history = [
    user('a'),
    reply('b', 40_000, [{ url: 'https://a.example/doc', text: longText }, { search: true }]),
    user('c', true),
    reply('d', 1_000, [{ url: 'https://b.example/', text: 'neu' }]),
    user('e'),
  ];

  it('marks outputs before the mark only, and leaves the stored messages alone', () => {
    const condensed = condenseHistory(history);
    const flags = condensed.flatMap((message) => message.parts.map((part) => (part as { output?: { condensed?: boolean } }).output?.condensed));
    expect(flags).toEqual([undefined, true, true, undefined, undefined, undefined, undefined, undefined]);
    expect((history[1]?.parts[0] as { output: { condensed?: boolean } }).output.condensed).toBeUndefined();
    expect(condenseHistory([user('a'), reply('b', 40_000, [{ url: 'https://a.example/', text: longText }])])).toEqual([
      user('a'),
      reply('b', 40_000, [{ url: 'https://a.example/', text: longText }]),
    ]);
  });

  it('lets condensed pages load again, by the requested and the final URL', () => {
    expect([...condensedPages(history)]).toEqual(['https://a.example/doc', 'https://a.example/doc#final']);
  });

  it('gives the model a stub for pages and titles with URLs for searches', () => {
    const stub = modelText({ ...page('https://a.example/doc', longText), condensed: true });
    expect(stub).toContain('Fetched https://a.example/doc earlier.');
    expect(stub).toContain('it loads without asking');
    expect(stub).toMatch(/<web-content-[0-9a-f]{16}>\nTitle: Handbuch\n<\/web-content-[0-9a-f]{16}>$/);
    expect(stub).not.toContain('xxx');

    const results = searchModelText({ ...search, condensed: true });
    expect(results).toContain('[1] Kaffee – Wikipedia https://de.wikipedia.org/wiki/Kaffee');
    expect(results).not.toContain('Ein Getränk.');
    expect(results).not.toContain('Warning');
    expect(results).toContain('its snippets were left out');
  });
});
