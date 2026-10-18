/**
 * Patterns of prompt injections in fetched content. Copied from the pi guard
 * (`crates/harness-core/pi/guard.ts`), which pi loads as a single file and
 * therefore cannot import from here; a test keeps both in step. Fixed
 * patterns only catch the blunt cases, so a hit is a warning, never a block.
 */
import type { WebFetchWarning } from '@harness/shared';

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
  [
    'html_comment',
    /<!--[\s\S]{0,400}?\b(ai|llm|assistant|agent|claude|gpt|deepseek|language model)\b[\s\S]{0,400}?-->/i,
  ],
  [
    'hide_from_user',
    /\b(do\s+not|don't|never)\s+(tell|inform|mention\s+(this\s+)?to|show)\s+(this\s+to\s+)?the\s+user\b/i,
  ],
];

/** Regular expressions of {@link RULES}, for the test that compares them with the pi guard. */
export const RULE_SOURCES: readonly string[] = RULES.map(([, pattern]) => pattern.source);

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
  zero_width: 'invisible zero-width characters',
};

/**
 * Looks for prompt injection patterns.
 *
 * @param texts - Texts to scan, e.g. the raw page and its readable form;
 *   comments only exist in the former, decoded entities only in the latter.
 * @returns The kinds of hits, each once; empty if nothing looks suspicious.
 */
export function scanForInjection(...texts: string[]): WebFetchWarning[] {
  const hits = new Set<WebFetchWarning>();
  for (const text of texts) {
    for (const [id, pattern] of RULES) {
      if (pattern.test(text)) {
        hits.add(id);
      }
    }
    // Tag characters spell out text no one sees ("ASCII smuggling").
    if (/[\u{E0000}-\u{E007F}]/u.test(text)) {
      hits.add('unicode_tags');
    }
    if (/[‪-‮⁦-⁩]/.test(text)) {
      hits.add('bidi_controls');
    }
    // A single zero-width character is common in ordinary text; many are not.
    if ((text.match(/[​‌⁠]/g) ?? []).length >= 3) {
      hits.add('zero_width');
    }
  }
  return [...hits];
}
