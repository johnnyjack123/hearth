/**
 * Picking the parts of a page that matter for what the model looks for:
 * the page is split into sections at its headings, the sections are ranked
 * by keywords (BM25, free) and, when an embedding model is available, by
 * meaning, and the two rankings are merged by Reciprocal Rank Fusion. The
 * model gets the best sections in page order plus an outline of all, so it
 * can ask for others.
 */
import { embed, type Helpers } from '../helpers/openrouter.ts';

/** A part of a page, numbered from 1 in page order. */
export interface Section {
  /** Number on the page, from 1. */
  index: number;
  /** The nearest heading above it, without `#`; empty before the first. */
  heading: string;
  /** Its Markdown. */
  text: string;
}

/** Longest section; longer ones are split at blank lines. ~400 tokens. */
const MAX_SECTION_CHARS = 1_600;

/**
 * Splits Markdown at its headings, and long parts at blank lines. Fenced
 * code blocks stay in one piece where they fit.
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

/** Cuts text longer than a section at blank lines (not inside code), else hard. */
function splitLong(text: string): string[] {
  if (text.length <= MAX_SECTION_CHARS) {
    return [text];
  }
  const paragraphs: string[] = [];
  let inFence = false;
  let buffer: string[] = [];
  for (const line of text.split('\n')) {
    if (/^\s*(```|~~~)/.test(line)) {
      inFence = !inFence;
    }
    if (!inFence && line.trim() === '' && buffer.length > 0) {
      paragraphs.push(buffer.join('\n'));
      buffer = [];
      continue;
    }
    buffer.push(line);
  }
  if (buffer.length > 0) {
    paragraphs.push(buffer.join('\n'));
  }
  const pieces: string[] = [];
  let piece = '';
  for (const paragraph of paragraphs) {
    // A heading alone says nothing; it stays with the text below it, even
    // if that makes the piece a little longer.
    const headingOnly = /^#{1,6}\s[^\n]*$/.test(piece);
    if (piece && !headingOnly && piece.length + paragraph.length + 2 > MAX_SECTION_CHARS) {
      pieces.push(piece);
      piece = '';
    }
    if (paragraph.length > MAX_SECTION_CHARS) {
      let rest = piece ? `${piece}\n\n${paragraph}` : paragraph;
      piece = '';
      while (rest.length > MAX_SECTION_CHARS) {
        // Cut at a space where one is near, not inside a word.
        const space = rest.lastIndexOf(' ', MAX_SECTION_CHARS);
        const cut = space > MAX_SECTION_CHARS / 2 ? space : MAX_SECTION_CHARS;
        pieces.push(rest.slice(0, cut));
        rest = rest.slice(cut);
      }
      piece = rest;
      continue;
    }
    piece = piece ? `${piece}\n\n${paragraph}` : paragraph;
  }
  if (piece) {
    pieces.push(piece);
  }
  return pieces.map((part) => part.trim()).filter(Boolean);
}

function terms(text: string): string[] {
  return text.toLowerCase().match(/[\p{L}\p{N}][\p{L}\p{N}._-]*[\p{L}\p{N}]|[\p{L}\p{N}]{2,}/gu) ?? [];
}

/**
 * BM25 score of each section for a query: words that are rare on the page
 * and frequent in a section count most. Good at names, versions, numbers;
 * blind to synonyms and other languages.
 *
 * @param sections - The sections.
 * @param query - What the model looks for.
 * @returns One score per section, 0 when it shares no word with the query.
 */
export function bm25Scores(sections: Section[], query: string): number[] {
  const k1 = 1.2;
  const b = 0.75;
  const docs = sections.map((section) => terms(`${section.heading} ${section.text}`));
  const average = docs.reduce((sum, doc) => sum + doc.length, 0) / Math.max(1, docs.length);
  const wanted = [...new Set(terms(query))];
  const frequency = new Map<string, number>();
  for (const doc of docs) {
    for (const term of new Set(doc)) {
      frequency.set(term, (frequency.get(term) ?? 0) + 1);
    }
  }
  return docs.map((doc) => {
    const counts = new Map<string, number>();
    for (const term of doc) {
      counts.set(term, (counts.get(term) ?? 0) + 1);
    }
    let score = 0;
    for (const term of wanted) {
      const count = counts.get(term) ?? 0;
      if (count === 0) {
        continue;
      }
      const df = frequency.get(term) ?? 0;
      const idf = Math.log(1 + (docs.length - df + 0.5) / (df + 0.5));
      score += (idf * count * (k1 + 1)) / (count + k1 * (1 - b + (b * doc.length) / (average || 1)));
    }
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
export function cosine(a: readonly number[], b: readonly number[]): number {
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

async function scoresFor(sections: Section[], focus: string, helpers: Helpers | null, signal: AbortSignal | undefined): Promise<number[]> {
  const keywords = bm25Scores(sections, focus);
  if (!helpers) {
    return fuseRankings([keywords]);
  }
  const candidates = sections
    .map((section, index) => ({ section, index, score: keywords[index] ?? 0 }))
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .slice(0, MAX_EMBEDDED_SECTIONS);
  const vectors = await embed(
    helpers,
    [QUERY_INSTRUCTION + focus, ...candidates.map(({ section }) => (section.heading ? `${section.heading}\n\n${section.text}` : section.text))],
    signal,
  );
  if (!vectors) {
    return fuseRankings([keywords]);
  }
  const [query, ...rest] = vectors;
  const meaning = sections.map(() => 0);
  candidates.forEach((candidate, place) => {
    meaning[candidate.index] = cosine(query ?? [], rest[place] ?? []);
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
