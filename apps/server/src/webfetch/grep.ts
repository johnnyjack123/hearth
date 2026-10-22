/**
 * Finding exact figures on a page without reading all of it: the model
 * names words that stand next to what it looks for, and gets only the
 * lines with them, with the heading above and, for table rows, the header
 * of their table.
 */

/** Most text returned, ~1,500 tokens. */
const GREP_BUDGET_CHARS = 6_000;
/** Most words the model may name. */
const MAX_TERMS = 20;
/** Longer lines (whole paragraphs) are cut down to the parts around matches. */
const MAX_LINE_CHARS = 600;
/** Characters kept on each side of a match in a long line. */
const CONTEXT_CHARS = 200;

/** What {@link grepPage} found. */
export interface GrepResult {
  /** The matching lines with their context, as Markdown. */
  text: string;
  /** How many lines matched. */
  lines: number;
  /** True if more lines matched than fit the budget. */
  truncated: boolean;
  /** Words left out as too short, see {@link grepTerms}. */
  ignored: string[];
}

/** Shorter words with a letter or digit ("W", "24", "0.") match nearly every line. */
const MIN_TERM_CHARS = 3;

/**
 * Splits what the model asked for into words or phrases. Backslashes go:
 * models write regular expressions out of habit ("0\.", "\$"), but the words
 * are matched literally. Words under three characters with a letter or
 * digit count only when nothing longer was given; symbols like "€" always
 * count.
 *
 * @param pattern - Words or phrases separated by `|`.
 * @returns The words to look for and the ones left out, lowercase.
 */
export function grepTerms(pattern: string): { terms: string[]; ignored: string[] } {
  const all = [...new Set(pattern.split('|').map((term) => term.replace(/\\/g, '').trim().toLowerCase()))].filter(Boolean).slice(0, MAX_TERMS);
  const tooShort = (term: string) => [...term].length < MIN_TERM_CHARS && /[\p{L}\p{N}]/u.test(term);
  const kept = all.filter((term) => !tooShort(term));
  return kept.length > 0 ? { terms: kept, ignored: all.filter(tooShort) } : { terms: all, ignored: [] };
}

/**
 * Builds the matcher for what the model asked for. The words are matched
 * literally and ignoring case: a regular expression from the model could
 * take the server's time with catastrophic backtracking.
 *
 * @param pattern - Words or phrases separated by `|`.
 * @returns A matcher, or null if the pattern holds nothing to look for.
 */
export function grepMatcher(pattern: string): RegExp | null {
  const { terms } = grepTerms(pattern);
  if (terms.length === 0) {
    return null;
  }
  return new RegExp(terms.map((term) => term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|'), 'giu');
}

/** A long line shortened to the stretches around its matches. */
function excerpt(line: string, matcher: RegExp): string {
  if (line.length <= MAX_LINE_CHARS) {
    return line;
  }
  const spans: [number, number][] = [];
  // matchAll starts where the last test() stopped.
  matcher.lastIndex = 0;
  for (const match of line.matchAll(matcher)) {
    const start = Math.max(0, match.index - CONTEXT_CHARS);
    const end = Math.min(line.length, match.index + match[0].length + CONTEXT_CHARS);
    const last = spans.at(-1);
    if (last && start <= last[1]) {
      last[1] = end;
    } else {
      spans.push([start, end]);
    }
  }
  return spans.map(([start, end]) => `${start > 0 ? '…' : ''}${line.slice(start, end)}${end < line.length ? '…' : ''}`).join(' ');
}

/**
 * Finds the lines of a page that contain any of the words.
 *
 * @param markdown - The readable text of the page.
 * @param pattern - Words or phrases separated by `|`.
 * @returns The matching lines with headings and table headers, or null if
 *   the pattern holds nothing to look for.
 */
export function grepPage(markdown: string, pattern: string): GrepResult | null {
  const matcher = grepMatcher(pattern);
  if (!matcher) {
    return null;
  }
  const lines = markdown.split('\n');
  const out: string[] = [];
  let used = 0;
  let matched = 0;
  let truncated = false;
  let heading: string | null = null;
  let headingShown = false;
  // Header and separator of the table the current line belongs to.
  let tableHeader: string[] | null = null;
  let tableShown = false;
  /** Adds a match with its context, all or nothing. */
  const add = (block: string[]): boolean => {
    const size = block.reduce((sum, line) => sum + line.length + 1, 0);
    if (used + size > GREP_BUDGET_CHARS && out.length > 0) {
      truncated = true;
      return false;
    }
    out.push(...block);
    used += size;
    return true;
  };
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index] ?? '';
    if (/^#{1,6}\s/.test(line)) {
      heading = line.trim();
      headingShown = false;
    }
    const isRow = line.trimStart().startsWith('|');
    if (!isRow) {
      tableHeader = null;
      tableShown = false;
    } else if (tableHeader === null) {
      tableHeader = /^\s*\|[\s:|-]+\|\s*$/.test(lines[index + 1] ?? '') ? [line, lines[index + 1] ?? ''] : [];
    }
    matcher.lastIndex = 0;
    if (!matcher.test(line)) {
      continue;
    }
    matched += 1;
    if (truncated) {
      continue;
    }
    const block: string[] = [];
    if (heading && !headingShown && heading !== line.trim()) {
      block.push('', heading);
    }
    // Figures in a table row mean little without its column names.
    const header = isRow && !tableShown && tableHeader && tableHeader.length > 0 ? tableHeader : [];
    block.push(...header);
    if (header[0] !== line) {
      block.push(isRow ? line : excerpt(line, matcher));
    }
    if (add(block)) {
      headingShown = true;
      tableShown = tableShown || isRow;
    }
  }
  return { text: out.join('\n').trim(), lines: matched, truncated, ignored: grepTerms(pattern).ignored };
}
