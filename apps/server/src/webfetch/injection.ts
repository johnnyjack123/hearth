/**
 * Patterns of prompt injections in fetched content. Copied from the pi guard
 * (`crates/harness-core/pi/guard.ts`), which pi loads as a single file and
 * therefore cannot import from here; a test runs both on the same texts.
 * Fixed patterns only catch the blunt cases, so a hit is a warning, never a
 * block.
 */
import type { InjectionFinding, WebFetchWarning } from '@harness/shared';

const RULES: readonly [id: WebFetchWarning, pattern: RegExp][] = [
  [
    'ignore_instructions',
    /\b(ignore|disregard|forget|override)\s+((all|any|the|your|of)\s+)*(previous|prior|above|earlier|preceding|system)\s+(instructions?|prompts?|messages?|rules|context)\b/i,
  ],
  // Case-sensitive on purpose: "You are now DAN" or "… in developer mode",
  // but not "You are now logged in".
  ['role_reassignment', /\b[Yy]ou\s+are\s+now\s+(?:(?:a|an|the|my)\s|(?:in\s+)?\w+\s+mode\b|[A-Z]{2,}\b)/],
  ['injected_instructions', /\b(new|updated|real|actual|hidden)\s+(system\s+)?instructions?\s*:/i],
  [
    'addressed_to_ai',
    /\b(dear|attention|note\s+to|message\s+(to|for)|instructions?\s+for)\s+(the\s+)?(ai|llm|assistant|agent|language\s+model|coding\s+agent)\b/i,
  ],
  ['addressed_to_ai', /\bif\s+you\s+are\s+an?\s+(ai|llm|language\s+model|assistant|agent)\b/i],
  ['role_markers', /<\|im_start\|>|<\|im_end\|>|<\|system\|>|<\/?\s*system\s*>|\[\/?INST\]|<<\/?SYS>>/i],
  // Within one comment: pages built with Svelte or Vue are full of empty
  // `<!---->` markers, and a match across them took visible text for a comment.
  [
    'html_comment',
    /<!--(?:(?!-->)[\s\S]){0,400}?\b(ai|llm|assistant|agent|claude|gpt|deepseek|language model)\b(?:(?!-->)[\s\S]){0,400}?-->/i,
  ],
  [
    'hide_from_user',
    /\b(do\s+not|don't|never)\s+(tell|inform|mention\s+(this\s+)?to|show)\s+(this\s+to\s+)?the\s+user\b/i,
  ],
];

/** Regular expressions of {@link RULES}, for the test that compares them with the pi guard. */
export const RULE_SOURCES: readonly string[] = RULES.map(([, pattern]) => pattern.source);

// Tag characters spell out text no one sees ("ASCII smuggling").
const TAGS = /[\u{E0000}-\u{E007F}]+/gu;
const BIDI = /[\u{202A}-\u{202E}\u{2066}-\u{2069}]/gu;
// Characters that render as nothing: zero-width space, non-joiner and
// joiner, word joiner, byte order mark and soft hyphen. Between words or
// list items they are line-break hints, common on generated pages, so they
// alone are no finding. They are one where they hide a phrase from the
// patterns ("ig<U+200B>nore previous instructions") or form a run long
// enough to encode a message.
const INVISIBLE = /[\u{AD}\u{200B}-\u{200D}\u{2060}\u{FEFF}]/gu;
const HAS_INVISIBLE = /[\u{AD}\u{200B}-\u{200D}\u{2060}\u{FEFF}]/u;
const INVISIBLE_RUN = /[\u{AD}\u{200B}-\u{200D}\u{2060}\u{FEFF}]{4,}/gu;

/** Characters of context on each side of a match. */
const CONTEXT = 60;
/** Characters of a match; longer ones lose their middle. */
const MAX_MATCH = 200;
/** Locations shown per kind, and in total. */
const PER_KIND = 3;
const MAX_FINDINGS = 12;
/** Matches looked at per pattern, so a page full of repeats stays cheap. */
const MAX_MATCHES = 50;

/** What each warning means, in English for the model. */
export const WARNING_TEXT: Record<WebFetchWarning, string> = {
  ignore_instructions: 'ignore-previous-instructions phrasing',
  role_reassignment: 'role reassignment',
  injected_instructions: 'injected instructions',
  addressed_to_ai: 'text addressed to an AI',
  role_markers: 'fake chat role markers',
  html_comment: 'instructions hidden in an HTML comment',
  hide_from_user: 'asks to hide something from the user',
  unicode_tags: 'invisible Unicode tag characters',
  bidi_controls: 'invisible bidi control characters',
  zero_width: 'invisible characters hiding text',
};

// Halves of surrogate pairs that a cut separated.
const LONE_SURROGATE = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/g;

function oneLine(text: string): string {
  return text.replace(LONE_SURROGATE, '').replace(/\s+/g, ' ');
}

/** The last `max` characters of `text`, from a word start if cut. */
function tail(text: string, max: number): string {
  const chars = Array.from(oneLine(text));
  if (chars.length <= max) {
    return chars.join('');
  }
  const cut = chars.slice(-max).join('');
  const space = cut.indexOf(' ');
  return `…${space >= 0 && space < max / 3 ? cut.slice(space + 1) : cut}`;
}

/** The first `max` characters of `text`, up to a word end if cut. */
function head(text: string, max: number): string {
  const chars = Array.from(oneLine(text));
  if (chars.length <= max) {
    return chars.join('');
  }
  const cut = chars.slice(0, max).join('');
  const space = cut.lastIndexOf(' ');
  return `${space > (max * 2) / 3 ? cut.slice(0, space) : cut}…`;
}

function shorten(text: string): string {
  const chars = Array.from(oneLine(text));
  if (chars.length <= MAX_MATCH) {
    return chars.join('');
  }
  return `${chars.slice(0, (MAX_MATCH * 2) / 3).join('')} … ${chars.slice(-MAX_MATCH / 3).join('')}`;
}

function isInvisible(code: number): boolean {
  return code === 0xad || (code >= 0x200b && code <= 0x200d) || code === 0x2060 || code === 0xfeff;
}

/**
 * `text` without invisible characters, and for each of its positions the
 * position in `text`; null if there is nothing to remove.
 */
function visibleText(text: string): { text: string; origin: Uint32Array } | null {
  if (!HAS_INVISIBLE.test(text)) {
    return null;
  }
  const origin = new Uint32Array(text.length + 1);
  let length = 0;
  for (let index = 0; index < text.length; index += 1) {
    if (!isInvisible(text.charCodeAt(index))) {
      origin[length] = index;
      length += 1;
    }
  }
  origin[length] = text.length;
  return { text: text.replace(INVISIBLE, ''), origin };
}

function* take<T>(items: Iterable<T>, max: number): Generator<T> {
  let count = 0;
  for (const item of items) {
    if (count >= max) {
      return;
    }
    count += 1;
    yield item;
  }
}

/**
 * Looks for prompt injection patterns and where they are.
 *
 * @param texts - Texts to scan, the most readable first, e.g. the page as
 *   Markdown and the raw HTML; comments only exist in the latter, decoded
 *   entities only in the former. A match found twice is listed once.
 * @returns Up to three locations per kind; empty if nothing looks suspicious.
 */
export function findInjections(...texts: string[]): InjectionFinding[] {
  const findings: InjectionFinding[] = [];
  const seen = new Set<string>();
  const perKind = new Map<WebFetchWarning, number>();

  /** Records a match unless its kind is full or the same text was seen. */
  function add(kind: WebFetchWarning, text: string, start: number, end: number): void {
    const count = perKind.get(kind) ?? 0;
    if (count >= PER_KIND || findings.length >= MAX_FINDINGS) {
      return;
    }
    const match = shorten(text.slice(start, end));
    const key = `${kind}\u0000${match.toLowerCase()}`;
    if (seen.has(key)) {
      return;
    }
    seen.add(key);
    perKind.set(kind, count + 1);
    findings.push({
      kind,
      before: tail(text.slice(Math.max(0, start - CONTEXT * 4), start), CONTEXT),
      match,
      after: head(text.slice(end, end + CONTEXT * 4), CONTEXT),
    });
  }

  function addAll(kind: WebFetchWarning, text: string, pattern: RegExp): void {
    for (const match of take(text.matchAll(pattern), MAX_MATCHES)) {
      add(kind, text, match.index, match.index + match[0].length);
    }
  }

  for (const text of texts) {
    // The patterns run on the text as it reads, so invisible characters
    // cannot break up a phrase; the finding shows them where they were.
    const visible = visibleText(text);
    const scanned = visible?.text ?? text;
    for (const [kind, pattern] of RULES) {
      for (const match of take(scanned.matchAll(new RegExp(pattern.source, `${pattern.flags}g`)), MAX_MATCHES)) {
        const from = match.index;
        const to = from + match[0].length;
        const start = visible ? (visible.origin[from] ?? from) : from;
        const end = visible ? (visible.origin[to - 1] ?? to - 1) + 1 : to;
        add(kind, text, start, end);
        if (end - start !== to - from) {
          add('zero_width', text, start, end);
        }
      }
    }
    addAll('unicode_tags', text, TAGS);
    addAll('bidi_controls', text, BIDI);
    addAll('zero_width', text, INVISIBLE_RUN);
  }
  return findings;
}

/**
 * The kinds of {@link findInjections}, each once.
 *
 * @param texts - See {@link findInjections}.
 * @returns The kinds in order of their first finding.
 */
export function scanForInjection(...texts: string[]): WebFetchWarning[] {
  return warningKinds(findInjections(...texts));
}

/**
 * The kinds of findings, each once.
 *
 * @param findings - Findings of {@link findInjections}.
 * @returns The kinds in order of their first finding.
 */
export function warningKinds(findings: readonly InjectionFinding[]): WebFetchWarning[] {
  return [...new Set(findings.map((finding) => finding.kind))];
}
