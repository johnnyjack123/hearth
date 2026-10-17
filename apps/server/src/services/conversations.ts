import type { AgentDataParts, ConversationSummary, MessageMetadata } from '@harness/shared';
import type { UIMessage } from 'ai';
import type { Kysely, Selectable } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import type { ConversationTable, DB } from '../db/schema.ts';

/**
 * Message type stored and exchanged with the web app.
 */
export type ChatMessage = UIMessage<MessageMetadata, AgentDataParts>;

type ConversationRow = Selectable<ConversationTable> & {
  linkDeviceId: string | null;
  linkDeviceName: string | null;
  linkProject: string | null;
  linkHarness: string | null;
};

function toSummary(row: ConversationRow): ConversationSummary {
  const { linkDeviceId, linkDeviceName, linkProject, linkHarness } = row;
  return {
    id: row.id,
    title: row.title,
    credentialId: row.credentialId,
    modelId: row.modelId,
    link:
      linkDeviceId && linkDeviceName && linkProject && linkHarness
        ? { deviceId: linkDeviceId, deviceName: linkDeviceName, project: linkProject, harness: linkHarness }
        : null,
    createdAt: row.createdAt,
    updatedAt: row.updatedAt,
  };
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
    ]);
}

/**
 * Lists a user's conversations, most recently active first.
 *
 * @param db - Open database.
 * @param userId - Owner.
 * @param limit - Maximum number of conversations.
 * @returns Conversation summaries.
 */
export async function listConversations(db: Kysely<DB>, userId: string, limit = 200): Promise<ConversationSummary[]> {
  const rows = await selectConversations(db)
    .where('conversation.userId', '=', userId)
    .orderBy('conversation.updatedAt', 'desc')
    .limit(limit)
    .execute();
  return rows.map(toSummary);
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
  return row ? toSummary(row) : undefined;
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
  return toSummary({ ...row, linkDeviceId: null, linkDeviceName: null, linkProject: null, linkHarness: null });
}

/**
 * Links a conversation to a project on a device, replacing an earlier link.
 *
 * @param db - Open database.
 * @param conversationId - Conversation, already checked for ownership.
 * @param link - Device (already checked for ownership), project and harness.
 */
export async function setLink(
  db: Kysely<DB>,
  conversationId: string,
  link: { deviceId: string; project: string; harness: string },
): Promise<void> {
  await db.transaction().execute(async (trx) => {
    await trx.deleteFrom('chatLink').where('conversationId', '=', conversationId).execute();
    await trx
      .insertInto('chatLink')
      .values({ conversationId, ...link, createdAt: Date.now() })
      .execute();
  });
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
      .select('id')
      .where('id', '=', message.id)
      .where('conversationId', '=', conversationId)
      .executeTakeFirst();
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
