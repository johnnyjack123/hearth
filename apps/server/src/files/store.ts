import { mkdirSync } from 'node:fs';
import { mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import type { ChatFile, ChatFileKind } from '@hearth/shared';
import type { Kysely, Selectable } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import type { DB, FileTable } from '../db/schema.ts';
import { HttpError } from '../http/errors.ts';

/** A stored file as the database has it. */
export type FileRow = Selectable<FileTable>;

/** Uploads no message refers to are removed after this long. */
const ORPHAN_AGE_MS = 24 * 60 * 60_000;

/** Cleanup runs at most this often. */
const SWEEP_INTERVAL_MS = 60 * 60_000;

const ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/**
 * The description of a stored file the browser and the device get.
 *
 * @param row - Database row.
 * @returns The file without its text.
 */
export function describeFile(row: FileRow): ChatFile {
  return {
    id: row.id,
    name: row.name,
    mediaType: row.mediaType,
    size: Number(row.size),
    kind: row.kind as ChatFileKind,
    ...(row.text !== null ? { chars: row.text.length } : {}),
    ...(row.truncated ? { truncated: true } : {}),
  };
}

/**
 * Uploads of the chats: a row in `file` and the bytes in `<dataDir>/files`.
 * Only the owner reads a file; a message attaches it to its chat, and the
 * chat's deletion removes it.
 */
export class FileStore {
  readonly #dir: string;
  #sweptAt = 0;

  /**
   * @param db - Open database.
   * @param dataDir - The server's data directory.
   */
  constructor(
    private readonly db: Kysely<DB>,
    dataDir: string,
  ) {
    this.#dir = path.join(dataDir, 'files');
    // A data directory the server cannot write must not keep it from
    // starting (2.11.: Docker had created the bind mount as root, and with
    // Postgres nothing had written there before); uploads say why instead.
    try {
      mkdirSync(this.#dir, { recursive: true });
    } catch (error) {
      console.warn(`[files] ${this.#dir} cannot be created, uploads will fail: ${(error as Error).message}`);
    }
  }

  #path(id: string): string {
    if (!ID.test(id)) {
      throw new Error(`not a file id: ${id}`);
    }
    return path.join(this.#dir, id);
  }

  /**
   * Stores an upload of `userId`, not yet in any chat.
   *
   * @param file - What was detected and extracted.
   * @param bytes - The contents.
   * @returns The stored file.
   */
  async save(
    file: { userId: string; name: string; mediaType: string; kind: ChatFileKind; text: string | null; truncated: boolean },
    bytes: Uint8Array,
  ): Promise<ChatFile> {
    const row: FileRow = {
      id: uuidv7(),
      userId: file.userId,
      conversationId: null,
      name: file.name,
      mediaType: file.mediaType,
      kind: file.kind,
      size: bytes.length,
      text: file.text,
      truncated: file.truncated ? 1 : 0,
      createdAt: Date.now(),
    };
    // The bytes first: a row without them would break every later request.
    const unwritable = () => new HttpError(503, 'storage_unavailable', `The server cannot write to ${this.#dir}`);
    try {
      await mkdir(this.#dir, { recursive: true });
    } catch {
      throw unwritable();
    }
    try {
      await writeFile(this.#path(row.id), bytes);
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code;
      throw code === 'EACCES' || code === 'EPERM' || code === 'EROFS' ? unwritable() : error;
    }
    await this.db.insertInto('file').values(row).execute();
    void this.sweep();
    return describeFile(row);
  }

  /**
   * @param userId - Who asks.
   * @param id - File id.
   * @returns The file if it exists and belongs to `userId`.
   */
  async find(userId: string, id: string): Promise<FileRow | null> {
    if (!ID.test(id)) {
      return null;
    }
    return (await this.db.selectFrom('file').selectAll().where('id', '=', id).where('userId', '=', userId).executeTakeFirst()) ?? null;
  }

  /**
   * @param ids - File ids.
   * @returns Their rows, in no particular order; unknown ids are left out.
   */
  async rows(ids: string[]): Promise<FileRow[]> {
    const valid = ids.filter((id) => ID.test(id));
    return valid.length === 0 ? [] : this.db.selectFrom('file').selectAll().where('id', 'in', valid).execute();
  }

  /**
   * @param userId - Whose files.
   * @param conversationIds - Chats.
   * @returns The files of those chats that belong to `userId`.
   */
  async ofConversations(userId: string, conversationIds: string[]): Promise<FileRow[]> {
    return conversationIds.length === 0
      ? []
      : this.db.selectFrom('file').selectAll().where('userId', '=', userId).where('conversationId', 'in', conversationIds).execute();
  }

  /**
   * @param id - File id.
   * @returns The stored bytes.
   */
  read(id: string): Promise<Buffer> {
    return readFile(this.#path(id));
  }

  /**
   * Attaches uploads to a chat when a message refers to them. A file of
   * another user or another chat is refused, so a message cannot reach
   * files by guessing ids.
   *
   * @param userId - The sender.
   * @param conversationId - The chat.
   * @param ids - Files the message refers to.
   * @returns Their rows, in the order of `ids`.
   * @throws {HttpError} 400 if one is unknown or not the sender's.
   */
  async attach(userId: string, conversationId: string, ids: string[]): Promise<FileRow[]> {
    if (ids.length === 0) {
      return [];
    }
    const rows = await this.rows(ids);
    const byId = new Map(rows.map((row) => [row.id, row]));
    const ordered = ids.map((id) => byId.get(id));
    if (ordered.some((row) => !row || row.userId !== userId || (row.conversationId !== null && row.conversationId !== conversationId))) {
      throw new HttpError(400, 'validation', 'The message refers to a file that does not exist');
    }
    await this.db
      .updateTable('file')
      .set({ conversationId })
      .where('id', 'in', ids)
      .where('conversationId', 'is', null)
      .execute();
    return ordered as FileRow[];
  }

  /**
   * Removes the bytes of a chat's files; call before deleting the chat,
   * whose deletion removes the rows.
   *
   * @param conversationId - The chat.
   */
  async removeConversation(conversationId: string): Promise<void> {
    const rows = await this.db.selectFrom('file').select('id').where('conversationId', '=', conversationId).execute();
    await Promise.all(rows.map((row) => rm(this.#path(row.id), { force: true })));
  }

  /**
   * Removes uploads that never made it into a message, and bytes without a
   * row (a deleted user, a crash between the two writes). Runs at most once
   * an hour; errors are only logged.
   */
  async sweep(): Promise<void> {
    const now = Date.now();
    if (now - this.#sweptAt < SWEEP_INTERVAL_MS) {
      return;
    }
    this.#sweptAt = now;
    try {
      await this.db
        .deleteFrom('file')
        .where('conversationId', 'is', null)
        .where('createdAt', '<', now - ORPHAN_AGE_MS)
        .execute();
      const names = (await readdir(this.#dir)).filter((name) => ID.test(name));
      const known = new Set<string>();
      // In slices: SQLite takes a limited number of parameters per query.
      for (let start = 0; start < names.length; start += 500) {
        for (const row of await this.rows(names.slice(start, start + 500))) {
          known.add(row.id);
        }
      }
      await Promise.all(names.filter((name) => !known.has(name)).map((name) => rm(this.#path(name), { force: true })));
    } catch (error) {
      console.warn('[files] cleanup failed:', error instanceof Error ? error.message : error);
    }
  }
}

const stores = new WeakMap<Kysely<DB>, FileStore>();

/**
 * The file store of a database, one per server.
 *
 * @param db - Open database.
 * @param dataDir - The server's data directory.
 * @returns The store.
 */
export function fileStore(db: Kysely<DB>, dataDir: string): FileStore {
  let store = stores.get(db);
  if (!store) {
    store = new FileStore(db, dataDir);
    stores.set(db, store);
  }
  return store;
}
