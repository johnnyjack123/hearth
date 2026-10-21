/**
 * Word forms for the keyword search over sections: lowercase, umlauts
 * folded, and reduced to a stem, so "geröstet" finds "Rösten" and
 * "caching" finds "cache". German uses CISTEM (Weissweiler and Fraser,
 * 2017), English the Porter stemmer; which one a page needs comes from its
 * function words.
 */
import { stemmer as porter } from 'stemmer';

/** Languages with a stemmer. */
export type StemLanguage = 'de' | 'en';

// Only words that are no words in the other language ("in", "an", "die" are).
const GERMAN = new Set(['der', 'das', 'und', 'ist', 'nicht', 'mit', 'auf', 'für', 'sich', 'des', 'dem', 'den', 'ein', 'eine', 'von', 'zu', 'auch', 'wird', 'werden', 'oder', 'bei', 'sind', 'wie']);
const ENGLISH = new Set(['the', 'and', 'is', 'of', 'to', 'that', 'for', 'with', 'on', 'are', 'this', 'be', 'as', 'by', 'or', 'from', 'it', 'not', 'can', 'which', 'you']);

/** Words counted to tell the language; enough for any page. */
const LANGUAGE_SAMPLE = 20_000;

/**
 * Tells German from English by counting function words.
 *
 * @param text - Text of the page.
 * @returns The language, or null for other languages, mixed or too little text.
 */
export function pageLanguage(text: string): StemLanguage | null {
  let german = 0;
  let english = 0;
  for (const word of (text.toLowerCase().match(/\p{L}+/gu) ?? []).slice(0, LANGUAGE_SAMPLE)) {
    german += GERMAN.has(word) ? 1 : 0;
    english += ENGLISH.has(word) ? 1 : 0;
  }
  if (german + english < 10) {
    return null;
  }
  return german >= english * 1.5 ? 'de' : english >= german * 1.5 ? 'en' : null;
}

/**
 * Function words of questions, folded. They stand in most sections, so in
 * a query they only add noise: "Wie vermehrt man …" ranked sections up for
 * "wie" and "man".
 */
const STOP_WORDS = new Set(
  (
    'der die das den dem des ein eine einer eines einem einen und oder aber wie was wer wo wann warum wieso weshalb welche welcher ' +
    'welches welchen man ich du er sie es wir ihr ist sind war waren wird werden wurde wurden hat haben hatte kann konnen muss mussen ' +
    'soll sollte darf mit von zu zum zur im in an am auf aus bei nach fur uber unter vor durch ohne um nicht auch noch nur schon so ' +
    'sehr sich mir mich dir dich uns ihm ihn ihnen gibt viel viele mehr ' +
    'the a an and or but how what who where when why which is are was were be been do does did can could should would will to of in ' +
    'on at for with from by about as it this that these those i you he she we they my your not no so very there here have has had get'
  ).split(' '),
);

/**
 * Whether a word is a function word without meaning for the search.
 *
 * @param word - A word, lowercase and folded.
 * @returns True for words like "wie", "man", "the".
 */
export function isStopWord(word: string): boolean {
  return STOP_WORDS.has(word);
}

/** Lowercase without umlauts: "Müller" and "Mueller" stay apart, "Müller" and "muller" do not. */
function fold(word: string): string {
  return word.toLowerCase().replace(/ä/g, 'a').replace(/ö/g, 'o').replace(/ü/g, 'u').replace(/ß/g, 'ss');
}

/**
 * The CISTEM stem of a German word: suffixes go one letter or pair at a
 * time from the end, a "ge" prefix goes too. This is the case-insensitive
 * variant: it also cuts a final "t" from nouns, so "geröstet" and "Rösten"
 * meet, and the model may write in any case.
 *
 * @param word - A German word.
 * @returns Its stem, lowercase and without umlauts.
 */
export function cistem(word: string): string {
  let stem = fold(word).replace(/^ge(.{4,})/, '$1');
  // Letter groups stand in for one character while suffixes are stripped.
  stem = stem.replace(/sch/g, '$$').replace(/ei/g, '%').replace(/ie/g, '&').replace(/(.)\1/g, '$1*');
  while (stem.length > 3) {
    if (stem.length > 5 && /(e[mr]|nd)$/.test(stem)) {
      stem = stem.slice(0, -2);
      continue;
    }
    if (/[tesn]$/.test(stem)) {
      stem = stem.slice(0, -1);
      continue;
    }
    break;
  }
  return stem.replace(/(.)\*/g, '$1$1').replace(/%/g, 'ei').replace(/&/g, 'ie').replace(/\$/g, 'sch');
}

const TOKEN = /[\p{L}\p{N}][\p{L}\p{N}._-]*[\p{L}\p{N}]|[\p{L}\p{N}]{2,}/gu;

function stem(word: string, language: StemLanguage | null): string {
  if (language === 'de') {
    return cistem(word);
  }
  return language === 'en' ? porter(fold(word)) : fold(word);
}

/** A word of a text: its stem for matching forms, the word itself for matching parts of compounds. */
export interface Term {
  /** Stem, lowercase and without umlauts. */
  stem: string;
  /** The word, lowercase and without umlauts. */
  word: string;
}

/**
 * Terms of a text for the keyword search. Versions and other words with
 * digits stay whole ("2.4.1"); names with dots, dashes or underscores stay
 * whole and add their parts ("fs.watch" also gives "watch").
 *
 * @param text - Any text.
 * @param language - Stemmer to use; null only lowercases and folds.
 * @returns The terms in order.
 */
export function analyze(text: string, language: StemLanguage | null): Term[] {
  const terms: Term[] = [];
  for (const token of text.match(TOKEN) ?? []) {
    if (/\p{N}/u.test(token)) {
      terms.push({ stem: fold(token), word: fold(token) });
      continue;
    }
    const parts = token.split(/[._-]+/).filter((part) => part.length >= 2);
    if (parts.length > 1) {
      terms.push({ stem: fold(token), word: fold(token) });
    }
    for (const part of parts) {
      terms.push({ stem: stem(part, language), word: fold(part) });
    }
  }
  return terms;
}
