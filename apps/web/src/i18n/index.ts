import { de } from './de.ts';
import { en } from './en.ts';
import type { Locale, MessageKey, MessageParams, Messages } from './types.ts';

export type { Locale, MessageKey, MessageParams, TranslateFn } from './types.ts';

const catalogs: Record<Locale, Messages> = { de, en };

/**
 * Resolves a key against a locale's catalog and fills placeholders.
 *
 * @param locale - Language to use.
 * @param key - Dot-separated message key.
 * @param params - Values for `{name}` placeholders; missing ones stay visible.
 * @returns The translated string, or the key itself if it does not exist,
 *   which makes missing translations obvious in the UI.
 */
export function translate(locale: Locale, key: MessageKey, params?: MessageParams): string {
  let node: unknown = catalogs[locale];
  for (const segment of key.split('.')) {
    node = (node as Record<string, unknown> | undefined)?.[segment];
  }
  if (typeof node !== 'string') {
    return key;
  }
  if (!params) {
    return node;
  }
  return node.replace(/\{(\w+)\}/g, (match, name: string) => (name in params ? String(params[name]) : match));
}

/**
 * Picks the initial locale from the browser when the user has not chosen one.
 *
 * @returns `de` for German browsers, otherwise `en`.
 */
export function detectLocale(): Locale {
  return navigator.language.toLowerCase().startsWith('de') ? 'de' : 'en';
}
