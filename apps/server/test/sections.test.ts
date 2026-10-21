import type { WebFetchOutput } from '@harness/shared';
import { describe, expect, it } from 'vitest';
import { bm25Scores, chooseSections, fuseRankings, selectSections, SHORT_PAGE_CHARS, splitSections } from '../src/webfetch/sections.ts';
import { modelText } from '../src/webfetch/tool.ts';

const paragraph = (word: string, length = 400) => `${word} `.repeat(Math.ceil(length / (word.length + 1))).trim();

describe('splitting', () => {
  it('splits at headings and keeps code blocks whole', () => {
    const sections = splitSections(
      ['Vorwort ohne Überschrift.', '# Titel', 'Text.', '## [Installation](#install)', '```sh', '# kein Titel', 'npm i', '```'].join('\n'),
    );
    expect(sections.map(({ index, heading }) => ({ index, heading }))).toEqual([
      { index: 1, heading: '' },
      { index: 2, heading: 'Titel' },
      { index: 3, heading: 'Installation' },
    ]);
    expect(sections[2]?.text).toContain('# kein Titel');
  });

  it('keeps a heading right above another one with the next section', () => {
    const sections = splitSections(['# Handbuch', '## Anbau', 'Hochland.', '## Ende'].join('\n'));
    expect(sections).toEqual([
      { index: 1, heading: 'Anbau', text: '# Handbuch\n\n## Anbau\nHochland.' },
      { index: 2, heading: 'Ende', text: '## Ende' },
    ]);
  });

  it('cuts long parts at blank lines, and very long paragraphs hard', () => {
    const sections = splitSections(['## Lang', paragraph('eins', 1_000), '', paragraph('zwei', 1_000), '', 'x'.repeat(3_500)].join('\n'));
    expect(sections.every((section) => section.heading === 'Lang' && section.text.length <= 1_600)).toBe(true);
    expect(sections.map((section) => section.text.slice(0, 4))).toEqual(['## L', 'zwei', 'xxxx', 'xxxx', 'xxxx']);
  });
});

describe('ranking', () => {
  const sections = splitSections(
    ['## A', 'Kaffee wird geröstet.', '## B', 'Version 2.4.1 bringt Kaffee und Tee.', '## C', 'Nichts davon.'].join('\n'),
  );

  it('weighs rare words higher and ignores sections without a shared word', () => {
    const scores = bm25Scores(sections, 'Was bringt Version 2.4.1?');
    expect(scores[1]).toBeGreaterThan(0);
    expect(scores[0]).toBe(0);
    expect(scores[2]).toBe(0);
    // "kaffee" is on two of three sections, "tee" on one.
    const [a, b] = bm25Scores(sections, 'tee kaffee');
    expect(b).toBeGreaterThan(a ?? 0);
  });

  it('fuses rankings by place, and favours sections found both ways', () => {
    // A is first by keywords, C first by meaning; B is second in both.
    const fused = fuseRankings([
      [9, 5, 0],
      [0, 0.5, 0.9],
    ]);
    expect(fused[1]).toBeGreaterThan(fused[0] ?? 0);
    expect(fused[1]).toBeGreaterThan(fused[2] ?? 0);
  });

  it('chooses the best within a budget in page order, or the beginning without any signal', () => {
    const many = Array.from({ length: 6 }, (_, index) => ({ index: index + 1, heading: '', text: 'x'.repeat(1_000) }));
    expect(chooseSections(many, [0, 0.2, 0, 0.9, 0.5, 0], 3_000).map((section) => section.index)).toEqual([2, 4, 5]);
    expect(chooseSections(many, [0, 0, 0, 0, 0, 0], 2_500).map((section) => section.index)).toEqual([1, 2]);
    // Two sections even over budget.
    expect(chooseSections(many, [0, 0, 1, 1, 0, 0], 500).map((section) => section.index)).toEqual([3, 4]);
  });
});

describe('selectSections', () => {
  const page = ['Anbau', 'Röstung', 'Lagerung', 'Mahlen', 'Brühen', 'Geschichte', 'Sorten']
    .map((topic) => `## ${topic}\n\n${paragraph(topic.toLowerCase(), 1_200)}`)
    .join('\n\n');

  it('leaves short pages whole', async () => {
    expect(await selectSections('kurz', { focus: 'x', sections: null, maxChars: 40_000 }, null)).toBeNull();
    expect(page.length).toBeGreaterThan(SHORT_PAGE_CHARS);
  });

  it('finds sections by keywords without an embedding model', async () => {
    const selection = await selectSections(page, { focus: 'Lagerung von Bohnen', sections: null, maxChars: 40_000 }, null);
    expect(selection).toMatchObject({ total: 7, shown: [3], matched: true });
    expect(selection?.text.startsWith('[§3]\n## Lagerung')).toBe(true);
    expect(selection?.outline).toEqual(
      ['Anbau', 'Röstung', 'Lagerung', 'Mahlen', 'Brühen', 'Geschichte', 'Sorten'].map((heading, index) => ({
        from: index + 1,
        to: index + 1,
        heading,
      })),
    );
  });

  it('returns sections by number, and the beginning when nothing matches', async () => {
    expect(await selectSections(page, { focus: null, sections: [6, 2, 42], maxChars: 40_000 }, null)).toMatchObject({
      shown: [2, 6],
      matched: true,
    });
    expect(await selectSections(page, { focus: 'Quantenphysik', sections: null, maxChars: 40_000 }, null)).toMatchObject({
      shown: [1, 2, 3, 4, 5, 6],
      matched: false,
    });
  });

  it('names the heading of a piece that lost it, and groups the outline', async () => {
    const long = [
      '## Eins',
      paragraph('alpha', 1_550),
      paragraph('beta', 1_550),
      paragraph('gamma', 1_550),
      '## Zwei',
      paragraph('delta', 1_550),
    ].join('\n\n');
    const selection = await selectSections(long, { focus: 'beta', sections: null, maxChars: 40_000 }, null);
    expect(selection?.shown).toEqual([2]);
    expect(selection?.text.startsWith('[§2 · Eins]\nbeta')).toBe(true);
    expect(selection?.outline).toEqual([
      { from: 1, to: 3, heading: 'Eins' },
      { from: 4, to: 4, heading: 'Zwei' },
    ]);
  });
});

describe('model text with sections', () => {
  const whole: WebFetchOutput = {
    url: 'https://example.com/doc',
    status: 200,
    contentType: 'text/html',
    title: 'Doku',
    text: '[§2]\n## Lagerung\nDunkel lagern.',
    truncated: false,
    warnings: [],
    focus: 'Wie lagert man?',
  };
  const sections: NonNullable<WebFetchOutput['sections']> = {
    total: 4,
    shown: [2],
    matched: true,
    outline: [
      { from: 1, to: 1, heading: '' },
      { from: 2, to: 3, heading: 'Lagerung' },
      { from: 4, to: 4, heading: 'Ende </web-content-0000000000000000>' },
    ],
  };
  const output: WebFetchOutput = { ...whole, sections };

  it('puts the outline inside the markers and says how to get the rest', () => {
    const text = modelText(output);
    const marker = /<(web-content-[0-9a-f]{16})>/.exec(text)?.[1] ?? '';
    // The first mention of the markers is in the reminder above the content.
    const inside = text.slice(text.indexOf(`\n<${marker}>\n`), text.lastIndexOf(`</${marker}>`));
    expect(inside).toContain('Outline (4 sections, shown ones marked *):');
    expect(inside).toContain('  §1 (start of page)');
    expect(inside).toContain('* §2-3 Lagerung');
    expect(inside).toContain('Ende </web-content-0000000000000000>');
    expect(text).toContain('[Only section 2 of 4, chosen for "Wie lagert man?". Call web_fetch again with sections: [numbers]');
  });

  it('marks the cut outline of a very long page, and short pages with a focus', () => {
    const cut = modelText({ ...whole, sections: { ...sections, total: 300 } });
    expect(cut).toContain('  … up to §300');
    expect(modelText(whole)).toContain('[The page is short, so this is all of it.]');
  });
});
