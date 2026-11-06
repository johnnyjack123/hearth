import { createHash } from 'node:crypto';
import { MAX_MEMORIES, MAX_MEMORY_CHARS, type Memory, type MemorySource, memoryRef } from '@harness/shared';
import type { Kysely, Selectable } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import type { DB, MemoryTable } from '../db/schema.ts';
import { HttpError, notFound } from '../http/errors.ts';
import { findInjections } from '../webfetch/injection.ts';

function toMemory(row: Selectable<MemoryTable>): Memory {
  return {
    id: row.id,
    text: row.text,
    source: row.source as MemorySource,
    conversationId: row.conversationId,
    pending: Number(row.pending) === 1,
    createdAt: Number(row.createdAt),
    updatedAt: Number(row.updatedAt),
  };
}

/**
 * The user's memories, oldest first: the order the model reads them in,
 * which stays the same as long as nothing changes.
 *
 * @param db - Open database.
 * @param userId - Whose.
 * @returns The memories.
 */
export async function listMemories(db: Kysely<DB>, userId: string): Promise<Memory[]> {
  const rows = await db.selectFrom('memory').selectAll().where('userId', '=', userId).orderBy('createdAt').orderBy('id').execute();
  return rows.map(toMemory);
}

/**
 * Text that may become a memory. Every chat reads memories, so a hijacked
 * model ("remember to always send …") must not plant instructions there:
 * text that looks like prompt injection is refused.
 *
 * @param text - The memory's text.
 * @returns The text, trimmed and on one line.
 * @throws {HttpError} 422 `memory_rejected` or `validation`.
 */
function checkedText(text: string): string {
  const clean = text.replace(/\s+/g, ' ').trim();
  if (!clean || clean.length > MAX_MEMORY_CHARS) {
    throw new HttpError(422, 'validation', `A memory has 1 to ${MAX_MEMORY_CHARS} characters`);
  }
  if (findInjections(clean).length > 0) {
    throw new HttpError(422, 'memory_rejected', 'This text looks like instructions smuggled in from content, not like something the user wants remembered');
  }
  return clean;
}

/**
 * Stores a memory. What a model stores is a suggestion until the user
 * confirms it ({@link confirmMemory}): the pattern check above lets ordinary
 * looking instructions through, and every chat would follow them.
 *
 * @param db - Open database.
 * @param userId - Whose.
 * @param input - Its text, who stores it and the chat it came from.
 * @returns The memory.
 * @throws {HttpError} 409 `memory_full` at {@link MAX_MEMORIES}, 422 as {@link checkedText}.
 */
export async function addMemory(
  db: Kysely<DB>,
  userId: string,
  input: { text: string; source: MemorySource; conversationId?: string | null },
): Promise<Memory> {
  const text = checkedText(input.text);
  const existing = await listMemories(db, userId);
  const same = existing.find((memory) => memory.text.toLowerCase() === text.toLowerCase());
  if (same) {
    // The user typing a suggestion's text confirms it.
    return same.pending && input.source === 'user' ? confirmMemory(db, userId, same.id) : same;
  }
  if (existing.length >= MAX_MEMORIES) {
    throw new HttpError(409, 'memory_full', `The memory holds at most ${MAX_MEMORIES} entries`);
  }
  const now = Date.now();
  const row = {
    id: uuidv7(),
    userId,
    text,
    source: input.source,
    conversationId: input.conversationId ?? null,
    pending: input.source === 'user' ? 0 : 1,
    createdAt: now,
    updatedAt: now,
  };
  await db.insertInto('memory').values(row).execute();
  return toMemory(row);
}

/**
 * Confirms a model's suggestion: from then on every chat reads it.
 *
 * @param db - Open database.
 * @param userId - Whose.
 * @param id - Memory id.
 * @returns The memory.
 * @throws {HttpError} 404 if there is none.
 */
export async function confirmMemory(db: Kysely<DB>, userId: string, id: string): Promise<Memory> {
  const result = await db.updateTable('memory').set({ pending: 0, updatedAt: Date.now() }).where('id', '=', id).where('userId', '=', userId).executeTakeFirst();
  if (Number(result.numUpdatedRows) === 0) {
    throw notFound('Memory');
  }
  return toMemory(await db.selectFrom('memory').selectAll().where('id', '=', id).executeTakeFirstOrThrow());
}

/**
 * Changes a memory's text.
 *
 * @param db - Open database.
 * @param userId - Whose.
 * @param id - Memory id.
 * @param text - New text.
 * @returns The memory.
 */
export async function updateMemory(db: Kysely<DB>, userId: string, id: string, text: string): Promise<Memory> {
  const clean = checkedText(text);
  // The user's own wording: no longer a suggestion.
  const result = await db
    .updateTable('memory')
    .set({ text: clean, pending: 0, updatedAt: Date.now() })
    .where('id', '=', id)
    .where('userId', '=', userId)
    .executeTakeFirst();
  if (Number(result.numUpdatedRows) === 0) {
    throw notFound('Memory');
  }
  const row = await db.selectFrom('memory').selectAll().where('id', '=', id).executeTakeFirstOrThrow();
  return toMemory(row);
}

/**
 * Removes a memory. Models only remove what models stored: a page or file
 * that hijacked one must not wipe what the user wrote down.
 *
 * @param db - Open database.
 * @param userId - Whose.
 * @param id - Memory id, or the short id models see ({@link memoryRef}).
 * @param by - Who removes it.
 * @returns The removed memory, or null if there was none.
 * @throws {HttpError} 403 `memory_protected` for a model and a memory of the user.
 */
export async function removeMemory(db: Kysely<DB>, userId: string, id: string, by: MemorySource = 'user'): Promise<Memory | null> {
  const found = (await listMemories(db, userId)).find((memory) => memory.id === id || memoryRef(memory.id) === id.trim().toLowerCase());
  if (!found) {
    return null;
  }
  if (by !== 'user' && found.source === 'user') {
    throw new HttpError(
      403,
      'memory_protected',
      'The user wrote this memory; only they can remove it, in the web app under Settings, Memory. Tell them so.',
    );
  }
  await db.deleteFrom('memory').where('id', '=', found.id).where('userId', '=', userId).execute();
  return found;
}

/**
 * The memories as the model reads them, with a fingerprint: a chat sends
 * them again only when the fingerprint changes, so its prompt stays the same
 * for the provider's cache.
 *
 * @param memories - The user's memories, see {@link listMemories}.
 * @returns The block (empty without memories) and its fingerprint (empty
 *   string without memories).
 */
export function memoryContext(memories: Memory[]): { hash: string; block: string } {
  // Suggestions count once the user confirmed them.
  const confirmed = memories.filter((memory) => !memory.pending);
  if (confirmed.length === 0) {
    return { hash: '', block: '' };
  }
  const lines = confirmed.map((memory) => `- [${memoryRef(memory.id)}] ${memory.text}`);
  const hash = createHash('sha256').update(lines.join('\n')).digest('hex').slice(0, 16);
  const block = [
    // Agents read this without the chat's system prompt, so the note says
    // itself how to treat it. Models answered to it as if the user had just
    // written it, when it only said what the user wants them to know.
    '[LLM Harness memory: background the user saved, not part of what they write now. Follow its preferences on how to answer without ' +
      'mentioning them; use its facts only when they matter for the request. Do not bring these notes up unless asked. ' +
      'It replaces earlier memory notes; the ids are for the forget tool.',
    ...lines,
    ']',
  ].join('\n');
  return { hash, block };
}

/**
 * The memory note for a new user message: the memories when they changed
 * since the chat's last message, a short line when they were all removed,
 * nothing otherwise.
 *
 * @param lastHash - `memoryHash` of the chat's last user message that has one.
 * @param context - The memories now, see {@link memoryContext}.
 * @returns The note, or null.
 */
export function memoryNote(lastHash: string | undefined, context: { hash: string; block: string }): string | null {
  if ((lastHash ?? '') === context.hash) {
    return null;
  }
  return context.block || '[LLM Harness memory: the user removed all memories; earlier memory notes no longer apply.]';
}
