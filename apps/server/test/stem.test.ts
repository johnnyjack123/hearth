import { describe, expect, it } from 'vitest';
import { bm25Scores, splitSections } from '../src/webfetch/sections.ts';
import { analyze, cistem, pageLanguage } from '../src/webfetch/stem.ts';

describe('word forms', () => {
  it('stems German words like CISTEM', () => {
    // Case-insensitive CISTEM cuts a final "t" from nouns too.
    expect(cistem('Speicherbehältern')).toBe('speicherbehal');
    expect(cistem('Grenzpostens')).toBe('grenzpo');
    expect(cistem('Ausgefeiltere')).toBe('ausgefeil');
    // Forms of one word meet.
    expect(cistem('geröstet')).toBe(cistem('Rösten'));
    expect(cistem('reinigt')).toBe(cistem('Reinigen'));
    expect(cistem('Kaffees')).toBe(cistem('Kaffee'));
  });

  it('tells German from English pages', () => {
    expect(pageLanguage('Der Kaffee wird geröstet und ist dann mit Wasser auf dem Tisch, das ist nicht schwer, wie man sieht, und für alle.')).toBe('de');
    expect(pageLanguage('The coffee is roasted and then it is served with water on the table, which is not hard to do for you or me.')).toBe('en');
    expect(pageLanguage('Kaffee Tee Wasser')).toBeNull();
  });

  it('keeps versions whole and splits names into parts', () => {
    const stems = (text: string, language: 'de' | 'en' | null) => analyze(text, language).map((term) => term.stem);
    expect(stems('Version 2.4.1 von fs.watch', 'en')).toEqual(['version', '2.4.1', 'von', 'fs.watch', 'fs', 'watch']);
    expect(stems('Caching cached caches', 'en')).toEqual(['cach', 'cach', 'cach']);
    expect(analyze('Kaffee-Röstung', 'de')).toEqual([
      { stem: 'kaffee-rostung', word: 'kaffee-rostung' },
      { stem: 'kaffee', word: 'kaffee' },
      { stem: 'rostung', word: 'rostung' },
    ]);
  });
});

describe('keyword search with word forms', () => {
  const page = splitSections(
    [
      '## Anbau',
      'Der Kaffee wächst in den Tropen, und die Pflanze ist nicht winterhart. Das ist für den Anbau wichtig.',
      '## Rösten',
      'Beim Rösten werden die Bohnen des Kaffees auf etwa 200 Grad erhitzt. Das ist der wichtigste Schritt, und er dauert nicht lange.',
      '## Lagerung',
      'Die Lagerung von Kaffee erfolgt kühl und dunkel. Das ist für das Aroma wichtig, und Luft ist der größte Feind.',
      '## Zubereitung',
      'Die Kaffeebohnen werden gemahlen und mit heißem Wasser zu Kaffee aufgegossen. Das ist für viele das Ritual am Morgen.',
    ].join('\n'),
  );
  const best = (query: string) => {
    const scores = bm25Scores(page, query);
    return page[scores.indexOf(Math.max(...scores))]?.heading;
  };

  it('finds other forms of a word', () => {
    expect(best('Wie wird Kaffee geröstet?')).toBe('Rösten');
    expect(best('Wie lagert man Kaffee?')).toBe('Lagerung');
  });

  it('finds parts of compounds, and compounds from their parts', () => {
    // "Bohnen" stands in "Kaffeebohnen" (Zubereitung) and alone (Rösten).
    const scores = bm25Scores(page, 'Kaffeebohnen');
    expect(scores[3]).toBeGreaterThan(0);
    expect(scores[1]).toBeGreaterThan(0);
    expect(scores[3]).toBeGreaterThan(scores[1] ?? 0);
  });
});
