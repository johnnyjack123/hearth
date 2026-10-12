import type { de } from './de.ts';

type Widen<T> = { readonly [K in keyof T]: T[K] extends string ? string : Widen<T[K]> };

/**
 * Shape every locale must implement: the German tree with string leaves.
 */
export type Messages = Widen<typeof de>;

type Paths<T> = {
  [K in keyof T & string]: T[K] extends string ? K : `${K}.${Paths<T[K]>}`;
}[keyof T & string];

/**
 * Dot-separated key of a single UI string, e.g. `chat.placeholder`.
 */
export type MessageKey = Paths<Messages>;

/**
 * Supported UI languages.
 */
export type Locale = 'de' | 'en';

/**
 * Values substituted for `{name}` placeholders.
 */
export type MessageParams = Record<string, string | number>;

/**
 * Translates a key into the active language.
 */
export type TranslateFn = (key: MessageKey, params?: MessageParams) => string;
