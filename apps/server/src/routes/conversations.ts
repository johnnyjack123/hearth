import {
  approvalDecisionSchema,
  chatRequestSchema,
  type ConversationDetail,
  createConversationSchema,
  linkConversationSchema,
  updateConversationSchema,
} from '@harness/shared';
import { Hono } from 'hono';
import type { AppDeps, AppEnv } from '../http/context.ts';
import { HttpError, notFound } from '../http/errors.ts';
import {
  type ChatMessage,
  createConversation,
  deleteConversation,
  discardFrom,
  findConversation,
  listConversations,
  loadMessages,
  removeLink,
  saveMessage,
  setLink,
  updateConversation,
} from '../services/conversations.ts';
import { findCredential, resolveCredential } from '../services/credentials.ts';
import { findDevice, loadDeviceProjects } from '../services/devices.ts';
import { streamAgent, transcript } from './agent.ts';
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
  const { secretBox, llm, hub } = deps;

  /** Rejects a project the device does not offer (as far as it reported). */
  async function assertOffered(deviceId: string, project: string, harness: string): Promise<void> {
    // The live list counts too: the stored one follows a moment later.
    const stored = (await loadDeviceProjects(db, [deviceId])).get(deviceId) ?? [];
    const offered = [...hub.status(deviceId).projects, ...stored].find((candidate) => candidate.name === project);
    if (!offered?.harnesses.includes(harness)) {
      throw new HttpError(409, 'device_offline', 'The device does not offer this project or harness');
    }
  }

  /** Rejects a message to a device that cannot answer now. */
  function assertOnline(deviceId: string): void {
    if (!hub.status(deviceId).online) {
      throw new HttpError(409, 'device_offline', 'The device is not connected');
    }
  }

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
      if (conversation.link) {
        hub.closeSession(conversation.link.deviceId, conversation.id);
      }
      await deleteConversation(db, conversation.id);
      return c.body(null, 204);
    })
    .put('/:id/link', async (c) => {
      const user = c.get('user');
      const conversation = await findConversation(db, user.id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      const input = linkConversationSchema.parse(await c.req.json());
      if (!(await findDevice(db, user.id, input.deviceId))) {
        throw notFound('Device');
      }
      await assertOffered(input.deviceId, input.project, input.harness);
      const old = conversation.link;
      if (old && (old.deviceId !== input.deviceId || old.project !== input.project || old.harness !== input.harness)) {
        hub.closeSession(old.deviceId, conversation.id);
      }
      await setLink(db, conversation.id, input);
      return c.json(await findConversation(db, user.id, conversation.id));
    })
    .delete('/:id/link', async (c) => {
      const conversation = await findConversation(db, c.get('user').id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      if (conversation.link) {
        hub.closeSession(conversation.link.deviceId, conversation.id);
        await removeLink(db, conversation.id);
      }
      return c.body(null, 204);
    })
    .post('/:id/approvals/:approvalId', async (c) => {
      const conversation = await findConversation(db, c.get('user').id, c.req.param('id'));
      if (!conversation?.link) {
        throw notFound('Conversation');
      }
      const id = Number(c.req.param('approvalId'));
      if (!Number.isSafeInteger(id) || id < 0) {
        throw notFound('Approval');
      }
      const { decision } = approvalDecisionSchema.parse(await c.req.json());
      hub.decide(conversation.link.deviceId, conversation.id, id, decision);
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
      const { link } = conversation;
      if (link) {
        // Checked before anything is stored, so an offline device leaves no
        // unanswered message behind.
        assertOnline(link.deviceId);
      }

      // A known message id means "regenerate" or "edit": the old branch is replaced.
      await discardFrom(db, conversation.id, userMessage.id);
      const previous = await loadMessages(db, conversation.id);
      const stored: ChatMessage = { ...userMessage, metadata: { createdAt: Date.now() } };
      await saveMessage(db, conversation.id, stored);
      await updateConversation(db, conversation.id, { credentialId: row.id, modelId: body.modelId });

      const model = llm.createModel(credential, body.modelId);
      const text = userMessage.parts.map((part) => (part.type === 'text' ? part.text : '')).join('\n');
      if (!conversation.title) {
        void assignTitle(db, conversation.id, model, text);
      }

      if (link) {
        const run = hub.startRun(link.deviceId, {
          type: 'session.prompt',
          session: conversation.id,
          project: link.project,
          harness: link.harness,
          conversation: { id: conversation.id, title: conversation.title },
          text,
          history: transcript(previous),
          model: { credentialId: row.id, modelId: body.modelId },
        });
        return streamAgent({
          db,
          conversationId: conversation.id,
          link,
          messages: [...previous, stored],
          run,
          selection: { credentialId: row.id, modelId: body.modelId },
          abortSignal: c.req.raw.signal,
        });
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
