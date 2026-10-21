import type { WebFetchOutput } from '@harness/shared';
import { describe, expect, it } from 'vitest';
import { grepMatcher, grepPage } from '../src/webfetch/grep.ts';
import { modelText } from '../src/webfetch/tool.ts';

const page = [
  '# Tarife',
  'Einleitung ohne Zahlen.',
  '## Preise',
  'Alle Preise verstehen sich inklusive MwSt.',
  '',
  '| Tarif | Preis pro Monat | Daten |',
  '| --- | --- | --- |',
  '| Basis | 9,99 € | 5 GB |',
  '| Plus | 19,99 € | 20 GB |',
  '',
  '## Kündigung',
  `${'Lange Rede über Fristen. '.repeat(40)}Die Kündigungsfrist beträgt einen Monat. ${'Noch mehr Text. '.repeat(40)}`,
].join('\n');

describe('grep', () => {
  it('takes words literally and ignores case', () => {
    expect(grepMatcher('a.b|(x')?.test('A.B')).toBe(true);
    expect(grepMatcher('a.b')?.test('axb')).toBe(false);
    expect(grepMatcher(' | ')).toBeNull();
  });

  it('returns matching lines with their heading and the header of their table', () => {
    const result = grepPage(page, '€');
    expect(result).toEqual({
      text: ['## Preise', '| Tarif | Preis pro Monat | Daten |', '| --- | --- | --- |', '| Basis | 9,99 € | 5 GB |', '| Plus | 19,99 € | 20 GB |'].join('\n'),
      lines: 2,
      truncated: false,
    });
  });

  it('shows a matching table header once, and cuts long paragraphs down to the match', () => {
    const header = grepPage(page, 'preis');
    expect(header?.text.match(/\| Tarif \|/g)).toHaveLength(1);
    // The heading, the sentence below it and the table header.
    expect(header?.lines).toBe(3);
    const notice = grepPage(page, 'Kündigungsfrist');
    expect(notice?.text).toMatch(/^## Kündigung\n….*Die Kündigungsfrist beträgt einen Monat\..*…$/s);
    expect(notice?.text.length).toBeLessThan(600);
  });

  it('stops at the budget and says so', () => {
    const many = Array.from({ length: 400 }, (_, index) => `Zeile ${index}: Preis ${index} €`).join('\n');
    const result = grepPage(many, '€');
    expect(result?.lines).toBe(400);
    expect(result?.truncated).toBe(true);
    expect(result?.text.length).toBeLessThanOrEqual(6_000);
  });

  it('tells the model what it got and what to do when nothing matched', () => {
    const base: WebFetchOutput = {
      url: 'https://tarife.example/',
      status: 200,
      contentType: 'text/html',
      title: 'Tarife',
      text: '| Basis | 9,99 € |',
      truncated: false,
      warnings: [],
    };
    expect(modelText({ ...base, grep: { pattern: '€', lines: 2, truncated: false } })).toContain(
      '[Only the 2 lines with "€", with the headings above them and the header of each table.',
    );
    expect(modelText({ ...base, text: '', grep: { pattern: 'Roaming', lines: 0, truncated: false } })).toContain(
      '[Nothing on the page matched "Roaming". Try other words, a focus, or load the page without grep.]',
    );
    expect(modelText({ ...base, grep: { pattern: '€', lines: null, truncated: false } })).toContain('[The page is short, so this is all of it.]');
  });
});
