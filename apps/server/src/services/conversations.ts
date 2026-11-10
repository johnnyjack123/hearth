import type { AgentDataParts, ConversationSummary, MessageMetadata } from '@hearth/shared';
import type { UIMessage } from 'ai';
import type { Kysely, Selectable } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import type { ConversationTable, DB } from '../db/schema.ts';
import { addCost } from './costs.ts';

/**
 * Message type stored and exchanged with the web app.
 */
export type ChatMessage = UIMessage<MessageMetadata, AgentDataParts>;

type ConversationRow = Selectable<ConversationTable> & {
  linkDeviceId: string | null;
  linkDeviceName: string | null;
  linkProject: string | null;
  linkHarness: string | null;
  linkAutosync: number | null;
};

function toSummary(row: ConversationRow, tags: string[] = []): ConversationSummary {
  const { linkDeviceId, linkDeviceName, linkProject, linkHarness, linkAutosync } = row;
  return {
    id: row.id,
    title: row.title,
    credentialId: row.credentialId,
    modelId: row.modelId,
    link:
      linkDeviceId && linkDeviceName && linkProject && linkHarness
        ? {
            deviceId: linkDeviceId,
            deviceName: linkDeviceName,
            project: linkProject,
            harness: linkHarness,
            autosync: linkAutosync === 1,
          }
        : null,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
    tags,
  };
}

/** The tags of the given chats, in the order they were put on. */
async function tagsOf(db: Kysely<DB>, conversationIds: string[]): Promise<Map<string, string[]>> {
  const tags = new Map<string, string[]>();
  if (conversationIds.length === 0) {
    return tags;
  }
  const rows = await db
    .selectFrom('conversationTag')
    .select(['conversationId', 'name'])
    .where('conversationId', 'in', conversationIds)
    .orderBy('createdAt')
    .orderBy('name')
    .execute();
  for (const row of rows) {
    tags.set(row.conversationId, [...(tags.get(row.conversationId) ?? []), row.name]);
  }
  return tags;
}

function selectConversations(db: Kysely<DB>) {
  return db
    .selectFrom('conversation')
    .leftJoin('chatLink', 'chatLink.conversationId', 'conversation.id')
    .leftJoin('device', 'device.id', 'chatLink.deviceId')
    .selectAll('conversation')
    .select([
      'chatLink.deviceId as linkDeviceId',
      'device.name as linkDeviceName',
      'chatLink.project as linkProject',
      'chatLink.harness as linkHarness',
      'chatLink.autosync as linkAutosync',
    ]);
}

/**
 * Lists a user's conversations, most recently active first: the latest
 * `limit`, and every tagged one besides, so the folders of the chat list
 * are complete.
 *
 * @param db - Open database.
 * @param userId - Owner.
 * @param limit - Maximum number of untagged conversations.
 * @returns Conversation summaries.
 */
export async function listConversations(db: Kysely<DB>, userId: string, limit = 200): Promise<ConversationSummary[]> {
  const latest = await selectConversations(db)
    .where('conversation.userId', '=', userId)
    .orderBy('conversation.updatedAt', 'desc')
    .limit(limit)
    .execute();
  const seen = new Set(latest.map((row) => row.id));
  const tagged = await selectConversations(db)
    .where('conversation.userId', '=', userId)
    .where('conversation.id', 'in', db.selectFrom('conversationTag').select('conversationTag.conversationId'))
    .execute();
  const rows = [...latest, ...tagged.filter((row) => !seen.has(row.id))].sort((a, b) => b.updatedAt - a.updatedAt);
  const tags = await tagsOf(
    db,
    rows.map((row) => row.id),
  );
  return rows.map((row) => toSummary(row, tags.get(row.id)));
}

/**
 * Loads a conversation if it belongs to the user.
 *
 * @param db - Open database.
 * @param userId - Expected owner.
 * @param id - Conversation id.
 * @returns The conversation, or undefined if missing or owned by someone else.
 */
export async function findConversation(
  db: Kysely<DB>,
  userId: string,
  id: string,
): Promise<ConversationSummary | undefined> {
  const row = await selectConversations(db)
    .where('conversation.id', '=', id)
    .where('conversation.userId', '=', userId)
    .executeTakeFirst();
  return row ? toSummary(row, (await tagsOf(db, [row.id])).get(row.id)) : undefined;
}

/**
 * Sets the tags of a chat. A name that differs from one of the user's tags
 * only in case takes that tag's spelling, so "arbeit" joins the folder
 * "Arbeit".
 *
 * @param db - Open database.
 * @param userId - Owner of the chat.
 * @param conversationId - Conversation, already checked for ownership.
 * @param names - The tags, trimmed.
 * @returns The tags as stored.
 */
export async function setTags(db: Kysely<DB>, userId: string, conversationId: string, names: string[]): Promise<string[]> {
  const known = new Map((await userTags(db, userId)).map((name) => [name.toLowerCase(), name]));
  const tags: string[] = [];
  for (const name of names) {
    const spelled = known.get(name.toLowerCase()) ?? name;
    if (!tags.some((tag) => tag.toLowerCase() === spelled.toLowerCase())) {
      tags.push(spelled);
    }
  }
  const now = Date.now();
  await db.transaction().execute(async (trx) => {
    const before = await trx.selectFrom('conversationTag').select(['name', 'createdAt']).where('conversationId', '=', conversationId).execute();
    await trx.deleteFrom('conversationTag').where('conversationId', '=', conversationId).execute();
    if (tags.length > 0) {
      // Tags already on the chat keep their place.
      const since = new Map(before.map((row) => [row.name, row.createdAt]));
      await trx
        .insertInto('conversationTag')
        .values(tags.map((name, index) => ({ conversationId, name, createdAt: since.get(name) ?? now + index })))
        .execute();
    }
  });
  return (await tagsOf(db, [conversationId])).get(conversationId) ?? [];
}

/** The distinct tags of a user's chats. */
async function userTags(db: Kysely<DB>, userId: string): Promise<string[]> {
  const rows = await db
    .selectFrom('conversationTag')
    .innerJoin('conversation', 'conversation.id', 'conversationTag.conversationId')
    .select('conversationTag.name')
    .distinct()
    .where('conversation.userId', '=', userId)
    .execute();
  return rows.map((row) => row.name);
}

/** The user's chats with a tag, compared without case. */
async function chatsTagged(db: Kysely<DB>, userId: string, name: string): Promise<{ conversationId: string; name: string }[]> {
  const rows = await db
    .selectFrom('conversationTag')
    .innerJoin('conversation', 'conversation.id', 'conversationTag.conversationId')
    .select(['conversationTag.conversationId', 'conversationTag.name'])
    .where('conversation.userId', '=', userId)
    .execute();
  return rows.filter((row) => row.name.toLowerCase() === name.toLowerCase());
}

/**
 * Renames a tag on all of a user's chats. Chats that carry both names keep
 * one; renaming to a tag in use merges the two folders.
 *
 * @param db - Open database.
 * @param userId - Owner.
 * @param from - The tag now.
 * @param to - Its new name.
 * @returns How many chats carry the tag.
 */
export async function renameTag(db: Kysely<DB>, userId: string, from: string, to: string): Promise<number> {
  const renamed = await chatsTagged(db, userId, from);
  if (renamed.length === 0) {
    return 0;
  }
  const target = (await userTags(db, userId)).find((name) => name.toLowerCase() === to.toLowerCase() && name.toLowerCase() !== from.toLowerCase()) ?? to;
  const already = new Set((await chatsTagged(db, userId, target)).map((row) => row.conversationId));
  await db.transaction().execute(async (trx) => {
    for (const row of renamed) {
      if (already.has(row.conversationId) && row.name !== target) {
        await trx.deleteFrom('conversationTag').where('conversationId', '=', row.conversationId).where('name', '=', row.name).execute();
      } else {
        await trx
          .updateTable('conversationTag')
          .set({ name: target })
          .where('conversationId', '=', row.conversationId)
          .where('name', '=', row.name)
          .execute();
      }
    }
  });
  return new Set([...renamed.map((row) => row.conversationId), ...already]).size;
}

/**
 * Takes a tag off all of a user's chats; the chats stay.
 *
 * @param db - Open database.
 * @param userId - Owner.
 * @param name - The tag.
 * @returns How many chats carried it.
 */
export async function removeTag(db: Kysely<DB>, userId: string, name: string): Promise<number> {
  const rows = await chatsTagged(db, userId, name);
  for (const row of rows) {
    await db.deleteFrom('conversationTag').where('conversationId', '=', row.conversationId).where('name', '=', row.name).execute();
  }
  return rows.length;
}

/**
 * Creates an empty conversation.
 *
 * @param db - Open database.
 * @param userId - Owner.
 * @param selection - Optional initial model selection.
 * @returns The new conversation.
 */
export async function createConversation(
  db: Kysely<DB>,
  userId: string,
  selection: { credentialId?: string | undefined; modelId?: string | undefined },
): Promise<ConversationSummary> {
  const now = Date.now();
  const row: Selectable<ConversationTable> = {
    id: uuidv7(),
    userId,
    title: null,
    credentialId: selection.credentialId ?? null,
    modelId: selection.modelId ?? null,
    createdAt: now,
    updatedAt: now,
  };
  await db.insertInto('conversation').values(row).execute();
  return toSummary({ ...row, linkDeviceId: null, linkDeviceName: null, linkProject: null, linkHarness: null, linkAutosync: null });
}

/**
 * Links a conversation to a project on a device, replacing an earlier link.
 *
 * @param db - Open database.
 * @param conversationId - Conversation, already checked for ownership.
 * @param link - Device (already checked for ownership), project and harness.
 * @param autosync - Take the agent's work over after every reply.
 */
export async function setLink(
  db: Kysely<DB>,
  conversationId: string,
  link: { deviceId: string; project: string; harness: string },
  autosync = false,
): Promise<void> {
  await db.transaction().execute(async (trx) => {
    await trx.deleteFrom('chatLink').where('conversationId', '=', conversationId).execute();
    await trx
      .insertInto('chatLink')
      .values({ conversationId, ...link, createdAt: Date.now(), autosync: autosync ? 1 : 0 })
      .execute();
  });
}

/**
 * Switches autosync of a linked conversation.
 *
 * @param db - Open database.
 * @param conversationId - Conversation, already checked for ownership.
 * @param autosync - The new setting.
 */
export async function setAutosync(db: Kysely<DB>, conversationId: string, autosync: boolean): Promise<void> {
  await db
    .updateTable('chatLink')
    .set({ autosync: autosync ? 1 : 0 })
    .where('conversationId', '=', conversationId)
    .execute();
}

/**
 * Removes a conversation's link; it becomes an ordinary chat again.
 *
 * @param db - Open database.
 * @param conversationId - Conversation, already checked for ownership.
 */
export async function removeLink(db: Kysely<DB>, conversationId: string): Promise<void> {
  await db.deleteFrom('chatLink').where('conversationId', '=', conversationId).execute();
}

/**
 * Changes fields of a conversation and bumps `updatedAt`.
 *
 * @param db - Open database.
 * @param id - Conversation id, already checked for ownership.
 * @param patch - Fields to change.
 */
export async function updateConversation(
  db: Kysely<DB>,
  id: string,
  patch: Partial<Pick<ConversationTable, 'title' | 'credentialId' | 'modelId'>>,
): Promise<void> {
  await db
    .updateTable('conversation')
    .set({ ...patch, updatedAt: Date.now() })
    .where('id', '=', id)
    .execute();
}

/**
 * Sets the title only if none exists yet, so a rename by the user is never
 * overwritten by a late auto-generated title.
 *
 * @param db - Open database.
 * @param id - Conversation id.
 * @param title - Generated title.
 */
export async function setTitleIfEmpty(db: Kysely<DB>, id: string, title: string): Promise<void> {
  await db.updateTable('conversation').set({ title }).where('id', '=', id).where('title', 'is', null).execute();
}

/**
 * Deletes a conversation and, via cascade, its messages.
 *
 * @param db - Open database.
 * @param id - Conversation id, already checked for ownership.
 */
export async function deleteConversation(db: Kysely<DB>, id: string): Promise<void> {
  await db.deleteFrom('conversation').where('id', '=', id).execute();
}

/**
 * Loads all messages of a conversation in order.
 *
 * @param db - Open database.
 * @param conversationId - Conversation id.
 * @returns Messages as stored by the chat endpoint.
 */
export async function loadMessages(db: Kysely<DB>, conversationId: string): Promise<ChatMessage[]> {
  const rows = await db
    .selectFrom('message')
    .select(['id', 'role', 'parts', 'metadata'])
    .where('conversationId', '=', conversationId)
    .orderBy('seq', 'asc')
    .execute();
  return rows.map((row) => ({
    id: row.id,
    role: row.role,
    parts: JSON.parse(row.parts) as ChatMessage['parts'],
    ...(row.metadata ? { metadata: JSON.parse(row.metadata) as MessageMetadata } : {}),
  }));
}

/**
 * Removes a message and everything after it. Used when the client re-sends an
 * existing user message: to regenerate the reply, or after editing it.
 *
 * @param db - Open database.
 * @param conversationId - Conversation id, already checked for ownership.
 * @param messageId - First message to remove.
 * @returns True if the message existed in this conversation.
 */
export async function discardFrom(db: Kysely<DB>, conversationId: string, messageId: string): Promise<boolean> {
  const row = await db
    .selectFrom('message')
    .select('seq')
    .where('conversationId', '=', conversationId)
    .where('id', '=', messageId)
    .executeTakeFirst();
  if (!row) {
    return false;
  }
  await db.deleteFrom('message').where('conversationId', '=', conversationId).where('seq', '>=', row.seq).execute();
  return true;
}

/**
 * Inserts a message, or replaces its parts and metadata if a message with the
 * same id already exists in the conversation (e.g. a continued assistant reply).
 *
 * @param db - Open database.
 * @param conversationId - Conversation id.
 * @param message - Message to store.
 */
export async function saveMessage(db: Kysely<DB>, conversationId: string, message: ChatMessage): Promise<void> {
  const parts = JSON.stringify(message.parts);
  const metadata = message.metadata ? JSON.stringify(message.metadata) : null;

  await db.transaction().execute(async (trx) => {
    const existing = await trx
      .selectFrom('message')
      .select(['id', 'metadata'])
      .where('id', '=', message.id)
      .where('conversationId', '=', conversationId)
      .executeTakeFirst();
    // Today's spending grows by what this save adds: a new reply, or a
    // stored one that a later step (Jev's assessment, a summary) made dearer.
    const before = existing?.metadata ? ((JSON.parse(existing.metadata) as ChatMessage['metadata'])?.usage?.cost ?? 0) : 0;
    const added = (message.metadata?.usage?.cost ?? 0) - before;
    if (added > 0) {
      const owner = await trx.selectFrom('conversation').select('userId').where('id', '=', conversationId).executeTakeFirst();
      if (owner) {
        await addCost(trx, owner.userId, added);
      }
    }
    if (existing) {
      await trx
        .updateTable('message')
        .set({ parts, metadata })
        .where('id', '=', message.id)
        .where('conversationId', '=', conversationId)
        .execute();
      return;
    }
    const last = await trx
      .selectFrom('message')
      .select((eb) => eb.fn.max('seq').as('seq'))
      .where('conversationId', '=', conversationId)
      .executeTakeFirst();
    await trx
      .insertInto('message')
      .values({
        id: message.id || uuidv7(),
        conversationId,
        role: message.role,
        parts,
        metadata,
        seq: Number(last?.seq ?? 0) + 1,
        createdAt: message.metadata?.createdAt ?? Date.now(),
      })
      .execute();
  });
}
