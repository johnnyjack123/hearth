import { describe, expect, it } from 'vitest';
import { htmlTitle, htmlToMarkdown, tidy } from '../src/webfetch/convert.ts';
import { splitSections } from '../src/webfetch/sections.ts';

/**
 * Pages and documents come from anyone. Patterns that backtrack over long
 * runs of one character took seconds to minutes on such input and blocked
 * the server (security review M5); these inputs must stay fast.
 */
const BIG = 1 << 20;

function timed<T>(work: () => T): { result: T; ms: number } {
  const started = performance.now();
  const result = work();
  return { result, ms: performance.now() - started };
}

describe('text handling on hostile input', () => {
  it('tidies a megabyte of spaces without a line break quickly', () => {
    const { result, ms } = timed(() => tidy(`a${' '.repeat(BIG)}b\nc   \n`));
    expect(ms).toBeLessThan(1_000);
    expect(result).toBe(`a${' '.repeat(BIG)}b\nc`);
  });

  it('finds the title among many unclosed title tags quickly', () => {
    const { result, ms } = timed(() => htmlTitle(`${'<title'.repeat(BIG / 6)}`));
    expect(ms).toBeLessThan(1_000);
    expect(result).toBeNull();
    expect(htmlTitle('<html><head><TITLE lang="de">Kaffee &amp; <b>Tee</b></title ></head>')).toBe('Kaffee & Tee');
    expect(htmlTitle(`<title>${'x'.repeat(5_000)}</title>`)).toBeNull();
  });

  it('drops scripts and styles, closed or not, quickly', () => {
    const { ms } = timed(() => htmlToMarkdown(`<p>Hallo</p>${'<script>'.repeat(BIG / 8)}`, new URL('https://example.com/')));
    expect(ms).toBeLessThan(2_000);
    const page = '<p>Vor</p><SCRIPT type="x">alert(1)</script ><style>p{}</style><p>Nach</p><svg><g/></svg><noscript>n</noscript>';
    expect(htmlToMarkdown(page, new URL('https://example.com/'))).toBe('Vor\n\nNach');
  });

  it('reads headings padded with spaces quickly', () => {
    const heading = `# Titel${' '.repeat(BIG / 4)}x`;
    const { ms } = timed(() => splitSections(`${heading}\n\nText`));
    expect(ms).toBeLessThan(1_000);
    expect(splitSections('# Titel ##\n\nText\n\n## Zweiter [Link](https://x.example) ¶\n\nMehr').map((section) => section.heading)).toEqual([
      'Titel',
      'Zweiter Link',
    ]);
  });
});
