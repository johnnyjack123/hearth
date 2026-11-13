/**
 * Text of messages for the chat search: what the user wrote and the visible
 * answers, without tool output, reasoning or notes. Stored folded in
 * `message.searchText`, so one `LIKE` finds words regardless of case and
 * accents in SQLite and Postgres alike.
 */

import type { Kysely } from 'kysely';
import type { DB } from '../db/schema.ts';

/** How one character compares in the search: lower case, no accents, ß as ss. */
function foldChar(char: string): string {
  return char.normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase().replace(/ß/g, 'ss');
}

/**
 * Text as the search compares it: lower case, accents and umlaut dots
 * dropped (ä → a), ß as ss.
 *
 * @param text - Any text.
 * @returns The folded text.
 */
export function fold(text: string): string {
  return text.normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase().replace(/ß/g, 'ss');
}

/**
 * Folds `text` and remembers where each folded character came from, so a
 * match in the folded text can be shown in the original.
 *
 * @param text - Any text.
 * @returns The folded text, and per folded character the start and end of
 *   its original character.
 */
function foldWithMap(text: string): { folded: string; from: number[]; to: number[] } {
  let folded = '';
  const from: number[] = [];
  const to: number[] = [];
  let offset = 0;
  for (const char of text) {
    const piece = foldChar(char);
    folded += piece;
    for (let unit = 0; unit < piece.length; unit += 1) {
      from.push(offset);
      to.push(offset + char.length);
    }
    offset += char.length;
  }
  return { folded, from, to };
}

/**
 * The visible text of a message: its text parts, one per line.
 *
 * @param parts - The stored parts, parsed or as JSON.
 * @returns The text; empty for messages without text.
 */
export function messageText(parts: unknown): string {
  const list: unknown = typeof parts === 'string' ? JSON.parse(parts) : parts;
  if (!Array.isArray(list)) {
    return '';
  }
  return list
    .filter((part): part is { type: 'text'; text: string } => part?.type === 'text' && typeof part.text === 'string')
    .map((part) => part.text)
    .join('\n');
}

/**
 * What the search compares for a message.
 *
 * @param parts - The stored parts, parsed or as JSON.
 * @returns The folded text.
 */
export function searchTextOf(parts: unknown): string {
  return fold(messageText(parts));
}

/**
 * Computes `searchText` for every message that has none (messages from
 * before the search existed), in batches. Folding is done in code because
 * it is not portable SQL.
 *
 * @param db - Open database, or the migration's transaction.
 */
export async function fillSearchText(db: Kysely<DB>): Promise<void> {
  let after = '';
  for (;;) {
    const rows = await db
      .selectFrom('message')
      .select(['id', 'parts'])
      .where('searchText', 'is', null)
      .where('role', '!=', 'system')
      .where('id', '>', after)
      .orderBy('id')
      .limit(500)
      .execute();
    if (rows.length === 0) {
      return;
    }
    const write = async (trx: Kysely<DB>) => {
      for (const row of rows) {
        await trx.updateTable('message').set({ searchText: searchTextOf(row.parts) }).where('id', '=', row.id).execute();
      }
    };
    // The migrator runs Postgres migrations in a transaction already;
    // SQLite gets one per batch instead of one commit per row.
    await (db.isTransaction ? write(db) : db.transaction().execute(write));
    after = rows.at(-1)?.id ?? after;
  }
}

/**
 * The words of a search, folded; very short ones are left out because they
 * match nearly everything.
 *
 * @param query - What the user typed.
 * @returns Up to eight distinct words of at least two characters.
 */
export function searchTerms(query: string): string[] {
  return [...new Set(fold(query).split(/\s+/).filter((term) => term.length >= 2))].slice(0, 8);
}

/** Characters around the first match shown before and after it. */
const BEFORE = 40;
const AFTER = 120;

/**
 * A short excerpt around the first search word in `text`, with the places
 * of all search words in it.
 *
 * @param text - The visible text of a message.
 * @param terms - Folded search words, see {@link searchTerms}.
 * @returns The excerpt (with … where it is cut) and [start, end) offsets of
 *   the words in it, sorted and without overlaps.
 */
export function snippet(text: string, terms: string[]): { text: string; marks: [number, number][] } {
  const { folded, from, to } = foldWithMap(text);
  const found: [number, number][] = [];
  for (const term of terms) {
    for (let at = folded.indexOf(term); at !== -1; at = folded.indexOf(term, at + term.length)) {
      found.push([from[at] ?? 0, to[at + term.length - 1] ?? text.length]);
    }
  }
  found.sort((a, b) => a[0] - b[0]);
  const first = found[0]?.[0] ?? 0;
  // Cut at spaces, so words stay whole.
  let start = Math.max(0, first - BEFORE);
  if (start > 0) {
    const space = text.indexOf(' ', start);
    start = space !== -1 && space < first ? space + 1 : start;
  }
  let end = Math.min(text.length, first + AFTER);
  if (end < text.length) {
    const space = text.lastIndexOf(' ', end);
    end = space > first ? space : end;
  }
  const prefix = start > 0 ? '… ' : '';
  const excerpt = prefix + text.slice(start, end).replace(/\s/g, ' ') + (end < text.length ? ' …' : '');
  const marks: [number, number][] = [];
  for (const [markStart, markEnd] of found) {
    if (markStart < start || markEnd > end) {
      continue;
    }
    const shifted: [number, number] = [markStart - start + prefix.length, markEnd - start + prefix.length];
    const last = marks.at(-1);
    if (last && shifted[0] <= last[1]) {
      last[1] = Math.max(last[1], shifted[1]);
    } else {
      marks.push(shifted);
    }
  }
  return { text: excerpt, marks };
}
