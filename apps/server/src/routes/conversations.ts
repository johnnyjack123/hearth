import {
  approvalDecisionSchema,
  assessmentRequestSchema,
  type ChatLink,
  chatRequestSchema,
  compactRequestSchema,
  type ConversationDetail,
  type EffortLevel,
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
import { type CredentialRow, findCredential, resolveCredential } from '../services/credentials.ts';
import { findDevice, loadDeviceProjects } from '../services/devices.ts';
import { getServerSettings, loadSearchBackend } from '../services/settings.ts';
import { ActiveStreams } from '../streams.ts';
import { FetchApprovals } from '../webfetch/approvals.ts';
import { DEFAULT_WEB_FETCH_CONFIG } from '../webfetch/tool.ts';
import { sharedCalculator } from '../calculator/engine.ts';
import { DEFAULT_SEARCH_ENDPOINTS } from '../websearch/providers.ts';
import { compactionRange, shouldCompact, summarize } from '../compact.ts';
import { shouldCondense } from '../condense.ts';
import { effortOptions } from '../effort.ts';
import { callUsage, withHelpers } from '../pricing.ts';
import { loadHelpers } from '../helpers/load.ts';
import { assessor, warnedContent, withAssessment } from '../assess.ts';
import { agentCompaction, type CompactionEvent, compactionCall, streamAgent, transcript, withCall } from './agent.ts';
import { assignTitle, chatNote, localDate, parseUserMessage, streamChat } from './chat.ts';
import { assertEndpointAllowed } from './providers.ts';

/** A takeover waits this long for the device (VM start, sync, checks). */
const TAKEOVER_TIMEOUT_MS = 5 * 60_000;
/** Longest an agent may take to summarize its context (it may start the VM first). */
const COMPACT_TIMEOUT_MS = 10 * 60_000;
/** pi's thinking level out of the box, for a linked chat without one. */
const AGENT_DEFAULT_THINKING: EffortLevel = 'medium';

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

/** Put in front of a message to an agent while research mode is on. */
const AGENT_RESEARCH_NOTE =
  '[LLM Harness: Research mode is on. Search the web with web_search for current or uncertain facts, ' +
  'read the most promising results with web_fetch and name your sources.]';

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
  const calculator = deps.calculator ?? sharedCalculator();
  // Replies being written, for a page that reloads in the middle of one.
  const streams = new ActiveStreams();

  /**
   * Has the agent of a linked chat summarize its context and stores pi's
   * summary with the last reply, like a takeover report.
   */
  async function compactAgent(conversationId: string, title: string | null, link: ChatLink): Promise<ChatMessage> {
    assertOnline(link.deviceId);
    const run = hub.startCompaction(link.deviceId, {
      type: 'session.compact',
      session: conversationId,
      project: link.project,
      harness: link.harness,
      conversation: { id: conversationId, title },
    });
    const timer = setTimeout(() => hub.abandonRun(conversationId, 'timeout'), COMPACT_TIMEOUT_MS);
    let result: CompactionEvent | null = null;
    let failure: string | null = null;
    try {
      for await (const event of run) {
        if (event.type === 'compaction' && event.state !== 'running') {
          result = { ...event, trigger: 'manual' };
        } else if (event.type === 'error') {
          failure = event.message;
        }
      }
    } finally {
      clearTimeout(timer);
    }
    if (result?.state === 'failed') {
      // Nothing to keep: the chat says why and stays as it was.
      throw new HttpError(502, 'device_failed', result.error ?? 'The agent wrote no summary');
    }
    if (!result) {
      if (failure === 'timeout') {
        throw new HttpError(504, 'device_timeout', 'The device did not finish the summary in time');
      }
      if (failure && LOST.has(failure)) {
        throw new HttpError(409, 'device_offline', 'The device disconnected');
      }
      throw new HttpError(502, 'device_failed', failure ?? 'The agent wrote no summary');
    }
    const last = (await loadMessages(db, conversationId)).at(-1);
    const part = { type: 'data-compaction' as const, id: `compaction-${uuidv7()}`, data: agentCompaction(result) };
    const call = result.state === 'done' ? compactionCall(result, last?.metadata?.modelId) : null;
    const message: ChatMessage =
      last?.role === 'assistant'
        ? {
            ...last,
            parts: [...last.parts, part],
            ...(call ? { metadata: { ...last.metadata, usage: withCall(last.metadata?.usage, call) } } : {}),
          }
        : {
            id: uuidv7(),
            role: 'assistant',
            parts: [part],
            metadata: {
              createdAt: Date.now(),
              agent: { project: link.project, harness: link.harness },
              ...(call ? { usage: withCall(undefined, call) } : {}),
            },
          };
    await saveMessage(db, conversationId, message);
    await updateConversation(db, conversationId, {});
    return message;
  }

  /** Jev for an agent reply: the assessor and the helpers whose cost joins the reply. */
  async function agentAssessor(userId: string, chat: CredentialRow, signal: AbortSignal) {
    const helpers = await loadHelpers(db, secretBox, userId, chat, deps.helpers);
    const assess = assessor(helpers, signal);
    return helpers && assess ? { assessor: assess, helpers } : null;
  }

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
      // Research mode needs a search provider; agents search through the
      // server as well (`/api/llm/proxy/_web/search`).
      const search = await loadSearchBackend(db, secretBox, searchEndpoints);
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
          ...(body.effort ? { effort: body.effort } : {}),
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
          // The agent learns about research mode with the message; the
          // server lets its searches through only while this reply runs.
          text: research ? `${AGENT_RESEARCH_NOTE}\n\n${text}` : text,
          // pi's levels are the chat's. pi keeps a level for the whole
          // session, so the chat's "default" has to be named, too.
          thinking: body.effort ?? AGENT_DEFAULT_THINKING,
          history: transcript(previous),
          model: { credentialId: row.id, modelId: body.modelId },
          autosync: link.autosync,
        }, research ? { question: text } : undefined);
        return streamAgent({
          db,
          conversationId: conversation.id,
          link,
          messages: [...previous, stored],
          run,
          selection: { credentialId: row.id, modelId: body.modelId },
          abortSignal: stop.signal,
          consumeSseStream: follow,
          assess: body.assess ? await agentAssessor(user.id, row, stop.signal) : null,
        });
      }

      const settings = await getServerSettings(db);
      const pricing = await deps.pricing.lookup(row.kind, row.baseUrl, body.modelId);
      // The window the last reply reported, else the catalog's.
      const contextWindow =
        previous.findLast((message) => message.metadata?.usage?.contextWindow)?.metadata?.usage?.contextWindow ?? pricing?.contextWindow;
      // Decided now and stored with the reply, so later requests start the same way.
      const compact = shouldCompact(previous, stored, settings.compact, contextWindow);
      return streamChat({
        db,
        conversationId: conversation.id,
        messages: [...previous, stored],
        model,
        modelId: body.modelId,
        credentialId: row.id,
        pricing,
        abortSignal: stop.signal,
        consumeSseStream: follow,
        webFetch: settings.webFetch ? { approvals: fetchApprovals, config: webFetchConfig } : null,
        search,
        research,
        calculator: settings.calculator ? calculator : null,
        helpers: await loadHelpers(db, secretBox, user.id, row, deps.helpers),
        title,
        assess: body.assess === true,
        effort: effortOptions(row.kind, row.baseUrl, body.effort),
        compaction: compact ? { keepRecentTokens: settings.compact.keepRecentTokens } : null,
      });
    })
    // Summarizes the chat now, on the user's request: everything but the
    // latest turn. The summary joins the last reply, like an automatic one.
    .post('/:id/compact', async (c) => {
      const user = c.get('user');
      const conversation = await findConversation(db, user.id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      if (streams.has(conversation.id)) {
        throw new HttpError(409, 'conflict', 'A reply is still being written');
      }
      if (conversation.link) {
        // The agent keeps its own history: pi summarizes it on the computer.
        return c.json(await compactAgent(conversation.id, conversation.title, conversation.link));
      }
      const body = compactRequestSchema.parse(await c.req.json());
      const row = await findCredential(db, user.id, body.credentialId);
      if (!row) {
        throw notFound('Credential');
      }
      await assertEndpointAllowed(db, user, row.kind, row.baseUrl);
      const messages = await loadMessages(db, conversation.id);
      const last = messages.at(-1);
      const range = compactionRange(messages, 0, true);
      if (!range || last?.role !== 'assistant') {
        throw new HttpError(409, 'conflict', 'Nothing to summarize yet');
      }
      let written: Awaited<ReturnType<typeof summarize>>;
      try {
        written = await summarize(llm.createModel(resolveCredential(secretBox, row), body.modelId), messages, range, 'manual', c.req.raw.signal);
      } catch (error) {
        throw new HttpError(502, 'provider_error', `The summary could not be written: ${error instanceof Error ? error.message : String(error)}`);
      }
      const pricing = await deps.pricing.lookup(row.kind, row.baseUrl, body.modelId);
      const call = { ...callUsage(written.usage, pricing, [], written.billed), model: body.modelId, purpose: 'compact' as const };
      const usage = last.metadata?.usage;
      const message: ChatMessage = {
        ...last,
        parts: [...last.parts, { type: 'data-compaction', id: `compaction-${uuidv7()}`, data: written.compaction }],
        metadata: {
          ...last.metadata,
          usage: usage ? withHelpers(usage, [call]) : { helpers: [call], ...(call.cost ? { cost: call.cost.total } : {}) },
        },
      };
      await saveMessage(db, conversation.id, message);
      await updateConversation(db, conversation.id, {});
      return c.json(message);
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
    // Jev's opinion on the prompt injection warning of one tool call, on the
    // user's request; stored with the tool's output, which the model never
    // gets back.
    .post('/:id/assessments', async (c) => {
      const user = c.get('user');
      const conversation = await findConversation(db, user.id, c.req.param('id'));
      if (!conversation) {
        throw notFound('Conversation');
      }
      const { messageId, toolCallId } = assessmentRequestSchema.parse(await c.req.json());
      const message = (await loadMessages(db, conversation.id)).find((candidate) => candidate.id === messageId);
      const index = message?.parts.findIndex((part) => 'toolCallId' in part && part.toolCallId === toolCallId) ?? -1;
      const part = message?.parts[index];
      const warned = part ? warnedContent(part) : null;
      if (!message || !part || !warned) {
        throw notFound('Warning');
      }
      const chat = conversation.credentialId ? await findCredential(db, user.id, conversation.credentialId) : undefined;
      const helpers = await loadHelpers(db, secretBox, user.id, chat ?? null, deps.helpers);
      if (!helpers) {
        throw new HttpError(409, 'conflict', 'Jev needs an OpenRouter key in the account');
      }
      const assessment = await assessor(helpers, c.req.raw.signal)?.(warned);
      if (!assessment) {
        throw new HttpError(502, 'provider_error', 'Jev did not answer');
      }
      const parts = message.parts.map((candidate, at) => (at === index ? withAssessment(candidate, assessment) : candidate));
      const usage = message.metadata?.usage;
      const updated: ChatMessage = {
        ...message,
        parts,
        ...(usage && message.metadata ? { metadata: { ...message.metadata, usage: withHelpers(usage, helpers.usage) } } : {}),
      };
      await saveMessage(db, conversation.id, updated);
      return c.json(updated);
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
