import type { MessageMetadata } from '@harness/shared';
import {
  APICallError,
  convertToModelMessages,
  createUIMessageStreamResponse,
  generateText,
  type LanguageModel,
  pruneMessages,
  streamText,
  toUIMessageStream,
  type ToolSet,
  validateUIMessages,
} from 'ai';
import type { Kysely } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import type { DB } from '../db/schema.ts';
import { HttpError } from '../http/errors.ts';
import { type ChatMessage, saveMessage, updateConversation } from '../services/conversations.ts';

const INSTRUCTIONS = [
  'You are a helpful assistant inside LLM Harness, a self-hosted chat application.',
  'Answer in the language the user writes in.',
  'Use Markdown for structure and fenced code blocks with a language tag for code.',
].join(' ');

const TITLE_INSTRUCTIONS =
  'Write a short title (at most six words) for a chat that starts with the message below. ' +
  'Use the language of the message. Reply with the title only, without quotes or punctuation at the end.';

const MAX_MESSAGE_CHARS = 100_000;
const MESSAGE_ID_PATTERN = /^[\w-]{1,100}$/;

/**
 * Validates the message sent by the client and reduces it to what users may
 * send: role `user` with text parts. Anything else (system or assistant
 * messages, tool parts) would let a client forge conversation history.
 *
 * @param raw - Untrusted message from the request body.
 * @returns The sanitized user message.
 * @throws {HttpError} 400 if the message is invalid or empty.
 */
export async function parseUserMessage(raw: unknown): Promise<ChatMessage> {
  let message: ChatMessage | undefined;
  try {
    [message] = await validateUIMessages<ChatMessage>({ messages: [raw] });
  } catch {
    throw new HttpError(400, 'validation', 'Invalid message');
  }
  if (!message || message.role !== 'user' || !MESSAGE_ID_PATTERN.test(message.id)) {
    throw new HttpError(400, 'validation', 'Only user messages can be sent');
  }
  const parts = message.parts.flatMap((part) =>
    part.type === 'text' && part.text.trim() ? [{ type: 'text' as const, text: part.text }] : [],
  );
  const length = parts.reduce((sum, part) => sum + part.text.length, 0);
  if (parts.length === 0 || length > MAX_MESSAGE_CHARS) {
    throw new HttpError(400, 'validation', `Message must contain 1 to ${MAX_MESSAGE_CHARS} characters of text`);
  }
  return { id: message.id, role: 'user', parts };
}

/** Longest tool output of an agent passed on to a model as context. */
const MAX_AGENT_OUTPUT_CHARS = 1_000;

/**
 * Turns the tool calls of an agent (from a time the chat was linked to an
 * environment) into text. A model without those tools would otherwise see
 * calls it cannot make; DeepSeek then writes its own call syntax into the
 * reply. Notices and gateway questions are display-only and dropped.
 *
 * @param messages - Stored messages.
 * @returns The same messages with agent tool calls as text parts.
 */
export function agentToolsAsText(messages: ChatMessage[]): ChatMessage[] {
  return messages.map((message) => {
    if (!message.parts.some((part) => part.type === 'dynamic-tool' || part.type.startsWith('data-'))) {
      return message;
    }
    const parts = message.parts.flatMap((part): ChatMessage['parts'] => {
      if (part.type.startsWith('data-')) {
        return [];
      }
      if (part.type !== 'dynamic-tool') {
        return [part];
      }
      const output =
        part.state === 'output-available'
          ? String((part.output as { text?: unknown } | undefined)?.text ?? '')
          : part.state === 'output-error'
            ? part.errorText
            : '';
      const shortened = output.length > MAX_AGENT_OUTPUT_CHARS ? `${output.slice(0, MAX_AGENT_OUTPUT_CHARS)}…` : output;
      return [
        {
          type: 'text',
          text: `[The agent in the linked environment ran ${part.toolName} ${JSON.stringify(part.input)}${shortened ? `; output: ${shortened}` : ''}]`,
        },
      ];
    });
    return { ...message, parts };
  });
}

/**
 * Derives a fallback title from the first user message.
 *
 * @param text - Text of the first message.
 * @returns At most 60 characters, cut at a word boundary where possible.
 */
export function fallbackTitle(text: string): string {
  const singleLine = text.replace(/\s+/g, ' ').trim();
  if (singleLine.length <= 60) {
    return singleLine;
  }
  const cut = singleLine.slice(0, 60);
  const lastSpace = cut.lastIndexOf(' ');
  return `${(lastSpace > 30 ? cut.slice(0, lastSpace) : cut).trimEnd()}…`;
}

function cleanTitle(raw: string): string {
  return raw
    .replace(/^#+\s*/, '')
    .replace(/[*_`]/g, '')
    .replace(/^["'„“«»]+|["'“”«»]+$/g, '')
    .replace(/[.!?:;]+$/, '')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 80);
}

/**
 * Sets a fallback title right away and replaces it with a model-generated one
 * once that arrives. Never throws: a failed title must not break the chat.
 *
 * @param db - Open database.
 * @param conversationId - Conversation without a title.
 * @param model - Model of the current request; titles cost the user's tokens,
 *   so no separate model is configured.
 * @param firstMessage - Text of the first user message.
 * @returns Resolves when the generated title was stored or discarded.
 */
export async function assignTitle(
  db: Kysely<DB>,
  conversationId: string,
  model: LanguageModel,
  firstMessage: string,
): Promise<void> {
  const fallback = fallbackTitle(firstMessage);
  await db.updateTable('conversation').set({ title: fallback }).where('id', '=', conversationId).execute();
  try {
    const { text } = await generateText({
      model,
      instructions: TITLE_INSTRUCTIONS,
      prompt: firstMessage.slice(0, 2_000),
      reasoning: 'minimal',
      abortSignal: AbortSignal.timeout(30_000),
    });
    const title = cleanTitle(text);
    if (title) {
      // Only replace our own fallback; the user may have renamed the chat meanwhile.
      await db
        .updateTable('conversation')
        .set({ title })
        .where('id', '=', conversationId)
        .where('title', '=', fallback)
        .execute();
    }
  } catch (error) {
    console.warn('[chat] title generation failed:', error instanceof Error ? error.message : error);
  }
}

/**
 * Turns a streaming error into a message for the chat UI. Provider errors are
 * passed through because they concern the user's own key or quota.
 *
 * @param error - Error raised while streaming.
 * @returns Text shown to the user.
 */
export function describeStreamError(error: unknown): string {
  if (APICallError.isInstance(error)) {
    const status = error.statusCode ? ` (HTTP ${error.statusCode})` : '';
    return `Provider error${status}: ${error.message}`;
  }
  console.error('[chat] stream failed', error);
  return 'The response could not be generated.';
}

/**
 * Options for {@link streamChat}.
 */
export interface StreamChatOptions {
  /** Open database. */
  db: Kysely<DB>;
  /** Conversation being continued, already checked for ownership. */
  conversationId: string;
  /** Stored history plus the new user message. */
  messages: ChatMessage[];
  /** Model to answer with. */
  model: LanguageModel;
  /** Model id, stored in the reply's metadata. */
  modelId: string;
  /** Credential id, stored in the reply's metadata. */
  credentialId: string;
  /** Aborts generation, e.g. when the client presses stop or disconnects. */
  abortSignal: AbortSignal;
}

/**
 * Streams the assistant's reply in the AI SDK UI message stream format and
 * stores it when the stream ends, including partial replies after an abort.
 *
 * @param options - See {@link StreamChatOptions}.
 * @returns A streaming HTTP response.
 */
export async function streamChat(options: StreamChatOptions): Promise<Response> {
  const { db, conversationId, messages, model, modelId, credentialId, abortSignal } = options;

  // Reasoning from earlier turns is display-only. Sending it back breaks when the
  // user switches providers mid-conversation (signatures are provider-specific).
  const modelMessages = pruneMessages({
    messages: await convertToModelMessages(agentToolsAsText(messages)),
    reasoning: 'all',
  });

  const result = streamText({ model, instructions: INSTRUCTIONS, messages: modelMessages, abortSignal });
  const startedAt = Date.now();
  const baseMetadata: MessageMetadata = { createdAt: startedAt, modelId, credentialId };

  const stream = toUIMessageStream<ToolSet, ChatMessage>({
    stream: result.stream,
    originalMessages: messages,
    generateMessageId: uuidv7,
    messageMetadata: ({ part }) => {
      if (part.type === 'start') {
        return baseMetadata;
      }
      if (part.type === 'finish') {
        return {
          ...baseMetadata,
          usage: { inputTokens: part.totalUsage.inputTokens, outputTokens: part.totalUsage.outputTokens },
        };
      }
      return undefined;
    },
    onEnd: async ({ responseMessage }) => {
      if (responseMessage.parts.length === 0) {
        return;
      }
      await saveMessage(db, conversationId, { ...responseMessage, metadata: { ...baseMetadata, ...responseMessage.metadata } });
      await updateConversation(db, conversationId, {});
    },
    onError: describeStreamError,
  });

  return createUIMessageStreamResponse({ stream });
}
