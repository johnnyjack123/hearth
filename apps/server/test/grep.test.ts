import type { WebFetchOutput } from '@hearth/shared';
import { describe, expect, it } from 'vitest';
import { grepMatcher, grepPage, grepTerms } from '../src/webfetch/grep.ts';
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
    expect(grepMatcher('abc.d|(x')?.test('ABC.D')).toBe(true);
    expect(grepMatcher('abc.d')?.test('abcxd')).toBe(false);
    expect(grepMatcher(' | ')).toBeNull();
  });

  it('drops words too short to mean anything, and backslashes of regular expressions', () => {
    // As DeepSeek wrote them in the benchmark of 22.10.2026.
    expect(grepTerms(String.raw`Pi 5|Leerlauf|W|24|0\.|€|\$`)).toEqual({ terms: ['pi 5', 'leerlauf', '€', '$'], ignored: ['w', '24', '0.'] });
    // Only short words: they are all there is to look for.
    expect(grepTerms('W|kW')).toEqual({ terms: ['w', 'kw'], ignored: [] });
    const result = grepPage(page, 'Preis|€|ab');
    expect(result?.ignored).toEqual(['ab']);
  });

  it('returns matching lines with their heading and the header of their table', () => {
    const result = grepPage(page, '€');
    expect(result).toEqual({
      text: ['## Preise', '| Tarif | Preis pro Monat | Daten |', '| --- | --- | --- |', '| Basis | 9,99 € | 5 GB |', '| Plus | 19,99 € | 20 GB |'].join('\n'),
      lines: 2,
      truncated: false,
      ignored: [],
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
