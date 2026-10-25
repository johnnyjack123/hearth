import {
  approvalDecisionSchema,
  type ChatLink,
  chatRequestSchema,
  type ConversationDetail,
  createConversationSchema,
  linkConversationSchema,
  type TakeoverReport,
  updateConversationSchema,
  updateLinkSchema,
  WEB_FETCH_DECISIONS,
  type WebFetchDecision,
} from '@harness/shared';
import { UI_MESSAGE_STREAM_HEADERS } from 'ai';
import { Hono } from 'hono';
import type { Kysely } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import type { DB } from '../db/schema.ts';
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
  setAutosync,
  setLink,
  updateConversation,
} from '../services/conversations.ts';
import { findCredential, resolveCredential } from '../services/credentials.ts';
import { findDevice, loadDeviceProjects } from '../services/devices.ts';
import { getServerSettings, loadSearchBackend } from '../services/settings.ts';
import { ActiveStreams } from '../streams.ts';
import { FetchApprovals } from '../webfetch/approvals.ts';
import { DEFAULT_WEB_FETCH_CONFIG } from '../webfetch/tool.ts';
import { DEFAULT_SEARCH_ENDPOINTS } from '../websearch/providers.ts';
import { shouldCondense } from '../condense.ts';
import { loadHelpers } from '../helpers/load.ts';
import { streamAgent, transcript } from './agent.ts';
import { assignTitle, chatNote, localDate, parseUserMessage, streamChat } from './chat.ts';
import { assertEndpointAllowed } from './providers.ts';

/** A takeover waits this long for the device (VM start, sync, checks). */
const TAKEOVER_TIMEOUT_MS = 5 * 60_000;

/** Ways a run ends without the device's own word. */
const LOST = new Set(['device_replaced', 'device_lost', 'device_protocol', 'chat_unlinked']);

/**
 * Adds a takeover report to the end of the chat: to the last reply if the
 * chat ends with one, so the card sits below the work it took over.
 *
 * @param db - Open database.
 * @param conversationId - Conversation, already checked for ownership.
 * @param link - The chat's environment.
 * @param report - The device's final report.
 * @returns The stored message.
 */
async function attachTakeover(db: Kysely<DB>, conversationId: string, link: ChatLink, report: TakeoverReport): Promise<ChatMessage> {
  const last = (await loadMessages(db, conversationId)).at(-1);
  const part = { type: 'data-takeover' as const, id: `takeover-${uuidv7()}`, data: report };
  const message: ChatMessage =
    last?.role === 'assistant'
      ? { ...last, parts: [...last.parts, part] }
      : {
          id: uuidv7(),
          role: 'assistant',
          parts: [part],
          metadata: { createdAt: Date.now(), agent: { project: link.project, harness: link.harness } },
        };
  await saveMessage(db, conversationId, message);
  return message;
}

/**
 * Routes for the signed-in user's conversations, mounted at `/api/conversations`.
 *
 * @param deps - Application dependencies.
 * @returns The router.
 */
export function conversationRoutes(deps: AppDeps) {
  const { db } = deps.database;
  const { secretBox, llm, hub } = deps;
  const webFetchConfig = { ...DEFAULT_WEB_FETCH_CONFIG, ...deps.webFetch };
  const searchEndpoints = { ...DEFAULT_SEARCH_ENDPOINTS, ...deps.webSearch };
  // Questions of web fetches in ordinary chats, answered through the same
  // route as the gateway's questions in linked chats.
  const fetchApprovals = new FetchApprovals();
  // Replies being written, for a page that reloads in the middle of one.
  const streams = new ActiveStreams();

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
      const { link, ...selection } = createConversationSchema.parse(await c.req.json().catch(() => ({})));
      if (selection.credentialId && !(await findCredential(db, user.id, selection.credentialId))) {
        throw notFound('Credential');
      }
      // Checked before anything is stored, so a refused link leaves no chat behind.
      if (link) {
        if (!(await findDevice(db, user.id, link.deviceId))) {
          throw notFound('Device');
        }
        await assertOffered(link.deviceId, link.project, link.harness);
      }
      const conversation = await createConversation(db, user.id, selection);
      if (!link) {
        return c.json(conversation, 201);
      }
      await setLink(db, conversation.id, link);
      return c.json(await findConversation(db, user.id, conversation.id), 201);
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
      // A reply no page reads any more would otherwise run on.
      streams.stop(conversation.id);
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
      const same = old !== null && old.deviceId === input.deviceId && old.project === input.project && old.harness === input.harness;
      if (old && !same) {
        hub.closeSession(old.deviceId, conversation.id);
      }
      // Autosync belongs to the project the chat works in; another one starts without.
      await setLink(db, conversation.id, input, same && old.autosync);
      return c.json(await findConversation(db, user.id, conversation.id));
    })
    .patch('/:id/link', async (c) => {
      const user = c.get('user');
      const conversation = await findConversation(db, user.id, c.req.param('id'));
      if (!conversation?.link) {
        throw notFound('Link');
      }
      const { autosync } = updateLinkSchema.parse(await c.req.json());
      await setAutosync(db, conversation.id, autosync);
      return c.json(await findConversation(db, user.id, conversation.id));
    })
    .post('/:id/takeover', async (c) => {
      const conversation = await findConversation(db, c.get('user').id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      const { link } = conversation;
      if (!link) {
        throw new HttpError(400, 'validation', 'The chat is not linked to an environment');
      }
      assertOnline(link.deviceId);
      const run = hub.startTakeover(link.deviceId, {
        type: 'session.takeover',
        session: conversation.id,
        project: link.project,
        harness: link.harness,
        conversation: { id: conversation.id, title: conversation.title },
      });
      const timer = setTimeout(() => hub.abandonRun(conversation.id, 'timeout'), TAKEOVER_TIMEOUT_MS);
      let report: TakeoverReport | null = null;
      let failure: string | null = null;
      try {
        for await (const event of run) {
          if (event.type === 'takeover') {
            report = event.report;
          } else if (event.type === 'error') {
            failure = event.message;
          }
        }
      } finally {
        clearTimeout(timer);
      }
      if (!report || report.state === 'running') {
        if (failure === 'timeout') {
          throw new HttpError(504, 'device_timeout', 'The device did not finish the takeover in time');
        }
        if (failure && LOST.has(failure)) {
          throw new HttpError(409, 'device_offline', 'The device disconnected');
        }
        // The device's own reason is German and shown as it is.
        throw new HttpError(502, 'device_failed', failure ?? 'The device sent no report');
      }
      const message = await attachTakeover(db, conversation.id, link, report);
      await updateConversation(db, conversation.id, {});
      return c.json(message);
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
      if (!conversation) {
        throw notFound('Conversation');
      }
      const id = Number(c.req.param('approvalId'));
      if (!Number.isSafeInteger(id) || id < 0) {
        throw notFound('Approval');
      }
      const { decision } = approvalDecisionSchema.parse(await c.req.json());
      if (fetchApprovals.has(conversation.id, id)) {
        // "Minutes" and "always" belong to the gateway; a web fetch only
        // knows once, this chat and no.
        if (!WEB_FETCH_DECISIONS.includes(decision as WebFetchDecision)) {
          throw new HttpError(400, 'validation', 'A web fetch can be allowed once or for this chat, or denied');
        }
        fetchApprovals.decide(conversation.id, id, decision);
        return c.body(null, 204);
      }
      if (!conversation.link) {
        throw notFound('Approval');
      }
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
      // Research mode belongs to ordinary chats with a search provider; the
      // flag on the message lets the chat open with the switch as it was.
      const search = link ? null : await loadSearchBackend(db, secretBox, searchEndpoints);
      const research = search !== null && body.research === true;
      const date = localDate(new Date());
      // Linked chats talk to an agent with its own prompt and get no notes.
      const note = link ? null : chatNote(previous, { date, research: search ? research : null });
      // Decided once and stored, so later requests condense at the same place.
      const condensed = !link && shouldCondense(previous, (await getServerSettings(db)).condense);
      const stored: ChatMessage = {
        ...userMessage,
        metadata: {
          createdAt: Date.now(),
          ...(link ? {} : { date }),
          ...(research ? { research } : {}),
          ...(note ? { note } : {}),
          ...(condensed ? { condensed } : {}),
        },
      };
      await saveMessage(db, conversation.id, stored);
      await updateConversation(db, conversation.id, { credentialId: row.id, modelId: body.modelId });

      const model = llm.createModel(credential, body.modelId);
      const text = userMessage.parts.map((part) => (part.type === 'text' ? part.text : '')).join('\n');
      // Only the stop route ends the reply: a closed or reloaded page is a
      // disconnect, and the page picks the reply up again (`GET …/stream`).
      const stop = new AbortController();
      const follow = (stream: ReadableStream<string>) => streams.add(conversation.id, stream, () => stop.abort());
      // Its cost joins the first reply of an ordinary chat.
      const title = conversation.title ? null : assignTitle(db, conversation.id, model, text);

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
          autosync: link.autosync,
        });
        return streamAgent({
          db,
          conversationId: conversation.id,
          link,
          messages: [...previous, stored],
          run,
          selection: { credentialId: row.id, modelId: body.modelId },
          abortSignal: stop.signal,
          consumeSseStream: follow,
        });
      }

      const settings = await getServerSettings(db);
      return streamChat({
        db,
        conversationId: conversation.id,
        messages: [...previous, stored],
        model,
        modelId: body.modelId,
        credentialId: row.id,
        pricing: await deps.pricing.lookup(row.kind, row.baseUrl, body.modelId),
        abortSignal: stop.signal,
        consumeSseStream: follow,
        webFetch: settings.webFetch ? { approvals: fetchApprovals, config: webFetchConfig } : null,
        search,
        research,
        helpers: await loadHelpers(db, secretBox, user.id, row, deps.helpers),
        title,
      });
    })
    .get('/:id/stream', async (c) => {
      const conversation = await findConversation(db, c.get('user').id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      // The reply being written from its start; the page rebuilds it from that.
      const stream = streams.resume(conversation.id);
      if (!stream) {
        return c.body(null, 204);
      }
      return new Response(stream.pipeThrough(new TextEncoderStream()), { headers: UI_MESSAGE_STREAM_HEADERS });
    })
    .post('/:id/stop', async (c) => {
      const conversation = await findConversation(db, c.get('user').id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      // The reply ends as after an error: what was written so far is stored.
      streams.stop(conversation.id);
      return c.body(null, 204);
    });
}
