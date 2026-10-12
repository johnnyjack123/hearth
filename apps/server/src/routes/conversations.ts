import {
  chatRequestSchema,
  type ConversationDetail,
  createConversationSchema,
  updateConversationSchema,
} from '@harness/shared';
import { Hono } from 'hono';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { notFound } from '../http/errors.ts';
import {
  type ChatMessage,
  createConversation,
  deleteConversation,
  discardFrom,
  findConversation,
  listConversations,
  loadMessages,
  saveMessage,
  updateConversation,
} from '../services/conversations.ts';
import { findCredential, resolveCredential } from '../services/credentials.ts';
import { assignTitle, parseUserMessage, streamChat } from './chat.ts';
import { assertEndpointAllowed } from './providers.ts';

/**
 * Routes for the signed-in user's conversations, mounted at `/api/conversations`.
 *
 * @param deps - Application dependencies.
 * @returns The router.
 */
export function conversationRoutes(deps: AppDeps) {
  const { db } = deps.database;
  const { secretBox, llm } = deps;

  return new Hono<AppEnv>()
    .get('/', async (c) => c.json(await listConversations(db, c.get('user').id)))
    .post('/', async (c) => {
      const user = c.get('user');
      const selection = createConversationSchema.parse(await c.req.json().catch(() => ({})));
      if (selection.credentialId && !(await findCredential(db, user.id, selection.credentialId))) {
        throw notFound('Credential');
      }
      return c.json(await createConversation(db, user.id, selection), 201);
    })
    .get('/:id', async (c) => {
      const conversation = await findConversation(db, c.get('user').id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      const detail: ConversationDetail<ChatMessage> = {
        ...conversation,
        messages: await loadMessages(db, conversation.id),
      };
      return c.json(detail);
    })
    .patch('/:id', async (c) => {
      const conversation = await findConversation(db, c.get('user').id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      const { title } = updateConversationSchema.parse(await c.req.json());
      await updateConversation(db, conversation.id, { title });
      return c.json({ ...conversation, title, updatedAt: Date.now() });
    })
    .delete('/:id', async (c) => {
      const conversation = await findConversation(db, c.get('user').id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      await deleteConversation(db, conversation.id);
      return c.body(null, 204);
    })
    .post('/:id/chat', async (c) => {
      const user = c.get('user');
      const conversation = await findConversation(db, user.id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      const body = chatRequestSchema.parse(await c.req.json());
      const row = await findCredential(db, user.id, body.credentialId);
      if (!row) {
        throw notFound('Credential');
      }
      await assertEndpointAllowed(db, user, row.kind, row.baseUrl);
      const credential = resolveCredential(secretBox, row);
      const userMessage = await parseUserMessage(body.message);

      // A known message id means "regenerate" or "edit": the old branch is replaced.
      await discardFrom(db, conversation.id, userMessage.id);
      const previous = await loadMessages(db, conversation.id);
      const stored: ChatMessage = { ...userMessage, metadata: { createdAt: Date.now() } };
      await saveMessage(db, conversation.id, stored);
      await updateConversation(db, conversation.id, { credentialId: row.id, modelId: body.modelId });

      const model = llm.createModel(credential, body.modelId);
      if (!conversation.title) {
        const text = userMessage.parts.map((part) => (part.type === 'text' ? part.text : '')).join('\n');
        void assignTitle(db, conversation.id, model, text);
      }

      return streamChat({
        db,
        conversationId: conversation.id,
        messages: [...previous, stored],
        model,
        modelId: body.modelId,
        credentialId: row.id,
        abortSignal: c.req.raw.signal,
      });
    });
}
