/**
 * Picking the parts of a page that matter for what the model looks for:
 * the page is split into sections at its headings, the sections are ranked
 * by keywords (BM25, free) and, when an embedding model is available, by
 * meaning, and the two rankings are merged by Reciprocal Rank Fusion. The
 * model gets the best sections in page order plus an outline of all, so it
 * can ask for others.
 */
import { embed, type Helpers } from '../helpers/openrouter.ts';
import { analyze, isStopWord, pageLanguage } from './stem.ts';

/** A part of a page, numbered from 1 in page order. */
export interface Section {
  /** Number on the page, from 1. */
  index: number;
  /** The nearest heading above it, without `#`; empty before the first. */
  heading: string;
  /** Its Markdown. */
  text: string;
}

/**
 * Longest section, ~500 tokens: small enough that a few fit the budget of a
 * focus and an embedding stands for one topic, large enough to keep a
 * paragraph with its context. Longer parts are halved until they fit.
 */
const MAX_SECTION_CHARS = 2_000;

/**
 * Splits Markdown at its headings, and halves parts that are too long
 * until each fits, so pages without headings (or with markup the
 * converter does not see as headings) get sections too. Fenced code blocks
 * stay in one piece where they fit.
 *
 * @param markdown - The page as Markdown.
 * @returns The sections in page order.
 */
export function splitSections(markdown: string): Section[] {
  const blocks: { heading: string; text: string }[] = [];
  let heading = '';
  let current: string[] = [];
  let inFence = false;
  // A heading right above another one (a page title over its first
  // chapter) joins the next section instead of standing alone.
  let carried = '';
  const flush = () => {
    const text = current.join('\n').trim();
    if (/^#{1,6}\s[^\n]*$/.test(text)) {
      carried = carried ? `${carried}\n\n${text}` : text;
    } else if (text) {
      blocks.push({ heading, text: carried ? `${carried}\n\n${text}` : text });
      carried = '';
    }
    current = [];
  };
  for (const line of markdown.split('\n')) {
    if (/^\s*(```|~~~)/.test(line)) {
      inFence = !inFence;
    }
    const match = !inFence ? /^(#{1,6})\s+(.+?)\s*#*\s*$/.exec(line) : null;
    if (match) {
      flush();
      heading = (match[2] ?? '')
        .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
        // Anchor links of docs pages ("#", "¶") are no part of the title.
        .replace(/\s*[#¶§]+$/, '')
        .trim()
        .slice(0, 120);
    }
    current.push(line);
  }
  flush();
  if (carried) {
    blocks.push({ heading, text: carried });
  }

  const sections: Section[] = [];
  for (const block of blocks) {
    for (const piece of splitLong(block.text)) {
      sections.push({ index: sections.length + 1, heading: block.heading, text: piece });
    }
  }
  return sections;
}

/** Where code blocks start and end, so cuts can avoid them. */
function fenceRanges(text: string): [number, number][] {
  const ranges: [number, number][] = [];
  let open = -1;
  let offset = 0;
  for (const line of text.split('\n')) {
    if (/^\s*(```|~~~)/.test(line)) {
      if (open < 0) {
        open = offset;
      } else {
        ranges.push([open, offset + line.length]);
        open = -1;
      }
    }
    offset += line.length + 1;
  }
  if (open >= 0) {
    ranges.push([open, text.length]);
  }
  return ranges;
}

/** Breaks to halve at, best first: paragraphs, lines, sentences, words. */
const BREAKS = [/\n[ \t]*\n/g, /\n/g, /[.!?…:;][)"'»“]?\s+/g, /\s+/g];

/**
 * Where to halve a text: the best kind of break nearest the middle, within
 * the middle half so both parts keep a useful size. Breaks inside code
 * blocks count only when nothing else is there.
 */
function halvingPoint(text: string): number {
  const middle = text.length / 2;
  const from = Math.floor(text.length / 4);
  const to = Math.ceil((text.length * 3) / 4);
  const fences = fenceRanges(text);
  for (const avoidCode of [true, false]) {
    for (const pattern of BREAKS) {
      let best = -1;
      pattern.lastIndex = from;
      for (let match = pattern.exec(text); match && match.index <= to; match = pattern.exec(text)) {
        const cut = match.index + match[0].length;
        if (avoidCode && fences.some(([start, end]) => match.index > start && match.index < end)) {
          continue;
        }
        if (best < 0 || Math.abs(cut - middle) < Math.abs(best - middle)) {
          best = cut;
        }
      }
      if (best > 0 && best < text.length) {
        return best;
      }
    }
  }
  // One long word: cut in the middle, but not inside a surrogate pair.
  const cut = Math.floor(middle);
  const code = text.charCodeAt(cut);
  return code >= 0xdc00 && code <= 0xdfff ? cut - 1 : cut;
}

/** Halves text longer than a section, again and again, until every part fits. */
function splitLong(text: string): string[] {
  if (text.length <= MAX_SECTION_CHARS) {
    return text ? [text] : [];
  }
  const cut = halvingPoint(text);
  return [...splitLong(text.slice(0, cut).trim()), ...splitLong(text.slice(cut).trim())];
}

/** Shortest word or stem that counts as part of a longer one ("bohnen" in "kaffeebohnen", "lager" in "lagerung"). */
const MIN_PART = 4;
/** A word part counts half as much as the word itself. */
const PART_WEIGHT = 0.5;
/** Text read to tell the page's language. */
const LANGUAGE_SAMPLE_CHARS = 50_000;

/** Whether one begins or ends the other, as parts of German compounds and stems of longer forms do. */
function isPart(a: string, b: string): boolean {
  const [short, long] = a.length <= b.length ? [a, b] : [b, a];
  return short.length >= MIN_PART && short !== long && (long.startsWith(short) || long.endsWith(short));
}

/**
 * BM25 score of each section for a query: words that are rare on the page
 * and frequent in a section count most. Words count in any form of the
 * page's language ("geröstet" for "Rösten") and as parts of compounds at
 * half weight ("Bohnen" in "Kaffeebohnen"). Good at names, versions,
 * numbers; blind to synonyms and other languages.
 *
 * @param sections - The sections.
 * @param query - What the model looks for.
 * @returns One score per section, 0 when it shares no word with the query.
 */
export function bm25Scores(sections: Section[], query: string): number[] {
  const k1 = 1.2;
  const b = 0.75;
  let sample = '';
  for (const section of sections) {
    if (sample.length >= LANGUAGE_SAMPLE_CHARS) {
      break;
    }
    sample += ` ${section.text}`;
  }
  const language = pageLanguage(sample);
  const analyzed = sections.map((section) => analyze(`${section.heading} ${section.text}`, language));
  const docs = analyzed.map((terms) => terms.map((term) => term.stem));
  const average = docs.reduce((sum, doc) => sum + doc.length, 0) / Math.max(1, docs.length);
  // Each stem of the page with the words it came from.
  const vocabulary = new Map<string, Set<string>>();
  for (const term of analyzed.flat()) {
    const words = vocabulary.get(term.stem) ?? new Set<string>();
    words.add(term.word);
    vocabulary.set(term.stem, words);
  }
  // For each word of the query: the stems of the page that stand for it.
  // Parts compare stems ("lager" in "lagerung") and whole words ("bohnen"
  // in "kaffeebohnen"; stemmed, that is "boh"). The exact stem and its
  // parts are separate terms, each with its own rarity: "kaffee" in
  // "Kaffeepflanzen" stands on every section of a coffee page and must not
  // make the rare "kaffeepflanz" look common.
  const all = analyze(query, language);
  const meaningful = all.filter((term) => !isStopWord(term.word));
  const queryTerms = new Map((meaningful.length > 0 ? meaningful : all).map((term) => [term.stem, term.word]));
  const wanted: { forms: Set<string>; weight: number }[] = [];
  for (const [stem, word] of queryTerms) {
    if (vocabulary.has(stem)) {
      wanted.push({ forms: new Set([stem]), weight: 1 });
    }
    const parts = new Set<string>();
    for (const [other, words] of vocabulary) {
      if (other !== stem && (isPart(stem, other) || [...words].some((candidate) => isPart(word, candidate)))) {
        parts.add(other);
      }
    }
    if (parts.size > 0) {
      wanted.push({ forms: parts, weight: PART_WEIGHT });
    }
  }
  const counts = docs.map((doc) => {
    const map = new Map<string, number>();
    for (const stem of doc) {
      map.set(stem, (map.get(stem) ?? 0) + 1);
    }
    return map;
  });
  const frequencies = wanted.map(({ forms }) => counts.filter((map) => [...forms].some((form) => map.has(form))).length);
  return docs.map((doc, index) => {
    const count = counts[index] ?? new Map<string, number>();
    let score = 0;
    wanted.forEach(({ forms, weight }, place) => {
      let tf = 0;
      for (const form of forms) {
        tf += count.get(form) ?? 0;
      }
      if (tf === 0) {
        return;
      }
      const df = frequencies[place] ?? 0;
      const idf = Math.log(1 + (docs.length - df + 0.5) / (df + 0.5));
      score += (weight * idf * tf * (k1 + 1)) / (tf + k1 * (1 - b + (b * doc.length) / (average || 1)));
    });
    return score;
  });
}

/**
 * Cosine similarity of two vectors.
 *
 * @param a - First vector.
 * @param b - Second vector.
 * @returns Between -1 and 1; 0 for empty or mismatched vectors.
 */
export function cosine(a: ArrayLike<number>, b: ArrayLike<number>): number {
  if (a.length === 0 || a.length !== b.length) {
    return 0;
  }
  let dot = 0;
  let normA = 0;
  let normB = 0;
  for (let index = 0; index < a.length; index += 1) {
    const x = a[index] ?? 0;
    const y = b[index] ?? 0;
    dot += x * y;
    normA += x * x;
    normB += y * y;
  }
  return normA === 0 || normB === 0 ? 0 : dot / Math.sqrt(normA * normB);
}

/** The usual constant of Reciprocal Rank Fusion: later ranks still count a little. */
const RRF_K = 60;

/**
 * Merges rankings by Reciprocal Rank Fusion: each ranking adds
 * 1 / (60 + place), so no scores need to be made comparable.
 *
 * @param rankings - Scores per section from each method; a section with
 *   score 0 in a ranking does not count there.
 * @returns The fused score per section.
 */
export function fuseRankings(rankings: number[][]): number[] {
  const length = rankings[0]?.length ?? 0;
  const fused = Array.from({ length }, () => 0);
  for (const scores of rankings) {
    const order = scores
      .map((score, index) => ({ score, index }))
      .filter((entry) => entry.score > 0)
      .sort((a, b) => b.score - a.score || a.index - b.index);
    order.forEach((entry, place) => {
      fused[entry.index] = (fused[entry.index] ?? 0) + 1 / (RRF_K + place + 1);
    });
  }
  return fused;
}

/**
 * The best sections within a budget, in page order. With no signal at all
 * (no shared word, no embeddings) the page's beginning is the best guess.
 *
 * @param sections - All sections.
 * @param scores - Fused score per section.
 * @param maxChars - Budget for the chosen text.
 * @param minSections - Sections taken even if they exceed the budget.
 * @returns The chosen sections, in page order.
 */
export function chooseSections(sections: Section[], scores: number[], maxChars: number, minSections = 2): Section[] {
  const ranked = sections
    .map((section, index) => ({ section, score: scores[index] ?? 0 }))
    .sort((a, b) => b.score - a.score || a.section.index - b.section.index);
  const useful = ranked[0] && ranked[0].score > 0 ? ranked.filter((entry) => entry.score > 0) : ranked;
  const chosen: Section[] = [];
  let used = 0;
  for (const { section } of useful) {
    if (chosen.length >= minSections && used + section.text.length > maxChars) {
      break;
    }
    chosen.push(section);
    used += section.text.length;
  }
  return chosen.sort((a, b) => a.index - b.index);
}

/** Pages up to this length come whole even with a focus: picking saves little. */
export const SHORT_PAGE_CHARS = 6_000;
/** Text of the sections chosen for a focus, ~2,000 tokens. */
const FOCUS_BUDGET_CHARS = 8_000;
/** Most sections sent to the embedding model; long pages send their best by keywords. */
const MAX_EMBEDDED_SECTIONS = 200;
/** Longest outline; on very long pages the rest is left out. */
const MAX_OUTLINE_GROUPS = 80;

/**
 * Qwen3-Embedding expects an instruction in front of queries, not in front
 * of documents; other embedding models take it as part of the query.
 */
const QUERY_INSTRUCTION = 'Instruct: Given a question, retrieve the passages of a web page that answer it\nQuery: ';

/** Consecutive sections under the same heading, as one line of the outline. */
export interface OutlineGroup {
  /** First section. */
  from: number;
  /** Last section. */
  to: number;
  /** Their heading, empty before the first heading of the page. */
  heading: string;
}

/** What {@link selectSections} gives the model instead of the whole page. */
export interface SectionSelection {
  /** The chosen sections, each behind a `[§n]` line. */
  text: string;
  /** How many sections the page has. */
  total: number;
  /** Numbers of the chosen sections. */
  shown: number[];
  /** All sections by heading, so the model can ask for others. */
  outline: OutlineGroup[];
  /** False when nothing matched the focus and the page's beginning stands in. */
  matched: boolean;
}

/** What the model asked for. */
export interface SectionRequest {
  /** What it looks for; null when it named sections only. */
  focus: string | null;
  /** Sections it asked for by number, from an earlier outline. */
  sections: number[] | null;
  /** Budget for sections asked for by number. */
  maxChars: number;
}

function outlineOf(sections: Section[]): OutlineGroup[] {
  const groups: OutlineGroup[] = [];
  for (const section of sections) {
    const last = groups.at(-1);
    if (last && last.heading === section.heading) {
      last.to = section.index;
    } else {
      groups.push({ from: section.index, to: section.index, heading: section.heading });
    }
  }
  return groups.slice(0, MAX_OUTLINE_GROUPS);
}

function render(sections: Section[]): string {
  return sections
    .map((section) => {
      // A piece after the first has lost its heading line; name it.
      const label = section.text.startsWith('#') || !section.heading ? `[§${section.index}]` : `[§${section.index} · ${section.heading}]`;
      return `${label}\n${section.text}`;
    })
    .join('\n\n');
}

/**
 * Lists of sources and links at the end of a page. They are full of topic
 * words, so keywords and embeddings both rank them high (Wikipedia's
 * "Einzelnachweise" came out on top for questions about coffee and sleep),
 * yet they rarely hold an answer.
 */
const BACK_MATTER =
  /^(einzelnachweise|anmerkungen|fußnoten|quellen|quellenangaben|literatur|weblinks|siehe auch|references|notes|footnotes|citations|sources|bibliography|external links|further reading|see also)$/i;
/** A focus with these words asks for exactly such lists. */
const ASKS_FOR_SOURCES = /quelle|literatur|nachweis|referenz|weblink|source|reference|citation|bibliograph|further reading/i;

async function scoresFor(sections: Section[], focus: string, helpers: Helpers | null, signal: AbortSignal | undefined): Promise<number[]> {
  const skip = ASKS_FOR_SOURCES.test(focus) ? () => false : (section: Section) => BACK_MATTER.test(section.heading.trim());
  // Left out of both rankings; the outline still lists them.
  const keywords = bm25Scores(sections, focus).map((score, index) => (skip(sections[index]!) ? 0 : score));
  if (!helpers) {
    return fuseRankings([keywords]);
  }
  const candidates = sections
    .map((section, index) => ({ section, index, score: keywords[index] ?? 0 }))
    .filter(({ section }) => !skip(section))
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .slice(0, MAX_EMBEDDED_SECTIONS);
  const vectors = await embed(
    helpers,
    [QUERY_INSTRUCTION + focus, ...candidates.map(({ section }) => (section.heading ? `${section.heading}\n\n${section.text}` : section.text))],
    signal,
  );
  const [query, ...rest] = vectors ?? [];
  if (!query) {
    return fuseRankings([keywords]);
  }
  // Sections of a failed batch have no vector and count by keywords only.
  const meaning = sections.map(() => 0);
  candidates.forEach((candidate, place) => {
    const vector = rest[place];
    meaning[candidate.index] = vector ? cosine(query, vector) : 0;
  });
  return fuseRankings([keywords, meaning]);
}

/**
 * Picks the sections of a page the model asked for: by number, or those
 * that match its focus by keywords and, with an embedding model, by
 * meaning.
 *
 * @param markdown - The readable text of the page.
 * @param request - See {@link SectionRequest}.
 * @param helpers - The user's helper models; without them keywords decide.
 * @param signal - Ends the embedding call with the chat request.
 * @returns The selection, or null when the page is short enough to come whole.
 */
export async function selectSections(
  markdown: string,
  request: SectionRequest,
  helpers: Helpers | null,
  signal?: AbortSignal,
): Promise<SectionSelection | null> {
  if (markdown.length <= SHORT_PAGE_CHARS) {
    return null;
  }
  const sections = splitSections(markdown);
  const wanted = new Set(request.sections ?? []);
  let chosen: Section[] = [];
  let matched = true;
  if (wanted.size > 0) {
    let used = 0;
    for (const section of sections) {
      if (wanted.has(section.index) && (chosen.length === 0 || used + section.text.length <= request.maxChars)) {
        chosen.push(section);
        used += section.text.length;
      }
    }
  }
  if (chosen.length === 0) {
    const scores = request.focus ? await scoresFor(sections, request.focus, helpers, signal) : sections.map(() => 0);
    matched = scores.some((score) => score > 0);
    chosen = chooseSections(sections, scores, FOCUS_BUDGET_CHARS);
  }
  return {
    text: render(chosen),
    total: sections.length,
    shown: chosen.map((section) => section.index),
    outline: outlineOf(sections),
    matched,
  };
}
