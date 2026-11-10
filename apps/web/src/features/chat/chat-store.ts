import { Chat } from '@ai-sdk/react';
import {
  type ConversationDetail,
  DEFAULT_TONE,
  DEFAULT_VERBOSITY,
  type EffortLevel,
  type ModelSelection,
  type Tone,
  type Verbosity,
} from '@hearth/shared';
import { DefaultChatTransport } from 'ai';
import { api, type ChatMessage } from '@/lib/api.ts';
import { autoAssessEnabled } from '@/lib/preferences.tsx';
import { queryClient } from '@/lib/query-client.ts';
import { queryKeys } from '@/lib/queries.ts';

/**
 * A live chat session plus the model its next request should use.
 */
export interface ChatEntry {
  /** AI SDK chat state machine; survives unmounting of the chat view. */
  chat: Chat<ChatMessage>;
  /** Read at request time, so model changes apply to the next message. */
  selection: { current: ModelSelection | null };
  /** Research mode of the next request, read at request time like the model. */
  research: { current: boolean };
  /** Thinking effort of the next request; null for the provider's default. */
  effort: { current: EffortLevel | null };
  /** Answer length of the next request. */
  verbosity: { current: Verbosity };
  /** Tone of the next request. */
  tone: { current: Tone };
  /**
   * The server may still be writing a reply this tab never saw, e.g. after
   * a reload; {@link resumeReply} picks it up once.
   */
  resumable: boolean;
}

// Chats live outside React on purpose. If useChat owned them, it would stop
// them on unmount — including React StrictMode's simulated unmount right after
// the first mount, which silently dropped the first message of a new chat. It
// also lets a reply keep streaming while the user looks at another chat.
const entries = new Map<string, ChatEntry>();

function refreshConversationList(): void {
  void queryClient.invalidateQueries({ queryKey: queryKeys.conversations, exact: true });
}

/**
 * Puts a message the server stored outside a chat request (a takeover
 * report) into the open chat and the cache: replaces the message with the
 * same id or appends it.
 *
 * @param conversationId - Conversation id.
 * @param message - The stored message.
 */
export function upsertMessage(conversationId: string, message: ChatMessage): void {
  const merge = (messages: ChatMessage[]) =>
    messages.some((candidate) => candidate.id === message.id)
      ? messages.map((candidate) => (candidate.id === message.id ? message : candidate))
      : [...messages, message];
  const entry = entries.get(conversationId);
  if (entry) {
    entry.chat.messages = merge(entry.chat.messages);
  }
  queryClient.setQueryData<ConversationDetail<ChatMessage>>(queryKeys.conversation(conversationId), (current) =>
    current ? { ...current, messages: merge(current.messages) } : current,
  );
}

/**
 * Returns the chat session of a conversation, creating it on first use.
 *
 * @param conversationId - Conversation id; also the chat id.
 * @param initialMessages - Stored messages, used only when the session is created.
 * @param initialSelection - Model for the first request, used only when the session is created.
 * @param initialResearch - Research mode for the first request; by default as
 *   in the newest user message.
 * @param initialEffort - Thinking effort for the first request; by default as
 *   in the newest user message.
 * @param initialVerbosity - Answer length for the first request; by default
 *   as in the newest user message.
 * @param initialTone - Tone for the first request; by default as in the
 *   newest user message.
 * @returns The session entry.
 */
export function getChat(
  conversationId: string,
  initialMessages: ChatMessage[],
  initialSelection: ModelSelection | null,
  initialResearch = lastResearch(initialMessages),
  initialEffort = lastEffort(initialMessages),
  initialVerbosity = lastVerbosity(initialMessages),
  initialTone = lastTone(initialMessages),
): ChatEntry {
  const existing = entries.get(conversationId);
  if (existing) {
    return existing;
  }

  const selection = { current: initialSelection };
  const research = { current: initialResearch };
  const effort = { current: initialEffort };
  const verbosity = { current: initialVerbosity };
  const tone = { current: initialTone };
  const chat: Chat<ChatMessage> = new Chat<ChatMessage>({
    id: conversationId,
    messages: initialMessages,
    transport: new DefaultChatTransport<ChatMessage>({
      api: `/api/conversations/${encodeURIComponent(conversationId)}/chat`,
      prepareReconnectToStreamRequest: () => ({ api: `/api/conversations/${encodeURIComponent(conversationId)}/stream` }),
      // The server owns the history; only the newest message is sent.
      prepareSendMessagesRequest: ({ messages }) => ({
        body: {
          message: messages.at(-1),
          credentialId: selection.current?.credentialId,
          modelId: selection.current?.modelId,
          research: research.current,
          assess: autoAssessEnabled(),
          ...(effort.current ? { effort: effort.current } : {}),
          ...(verbosity.current !== DEFAULT_VERBOSITY ? { verbosity: verbosity.current } : {}),
          ...(tone.current !== DEFAULT_TONE ? { tone: tone.current } : {}),
        },
      }),
    }),
    onFinish: () => {
      queryClient.setQueryData<ConversationDetail<ChatMessage>>(queryKeys.conversation(conversationId), (current) =>
        current ? { ...current, messages: chat.messages } : current,
      );
      refreshConversationList();
      // The server generates the title in the background, shortly after the reply.
      window.setTimeout(refreshConversationList, 2500);
    },
  });

  // A reply runs while the newest stored message is the user's: the server
  // stores the reply when it ends.
  const entry = { chat, selection, research, effort, verbosity, tone, resumable: initialMessages.at(-1)?.role === 'user' };
  entries.set(conversationId, entry);
  return entry;
}

/**
 * Picks up a reply the server is still writing for a session this tab just
 * created, e.g. after a reload: the server sends it again from its start.
 * Runs once per session; a session that streams in this tab already has
 * the reply.
 *
 * @param conversationId - Conversation id.
 */
export async function resumeReply(conversationId: string): Promise<void> {
  const entry = entries.get(conversationId);
  if (!entry?.resumable) {
    return;
  }
  entry.resumable = false;
  await entry.chat.resumeStream();
  // The reply may have ended between loading the chat and asking for it.
  if (entry.chat.status === 'ready' && entry.chat.messages.at(-1)?.role === 'user') {
    const fresh = await api.conversations.get(conversationId).catch(() => null);
    if (fresh && fresh.messages.length > entry.chat.messages.length) {
      entry.chat.messages = fresh.messages;
      queryClient.setQueryData(queryKeys.conversation(conversationId), fresh);
    }
  }
}

/**
 * Stops the reply of a conversation. Closing the page does not, so the
 * server is told; then this tab stops reading.
 *
 * @param conversationId - Conversation id.
 */
export async function stopReply(conversationId: string): Promise<void> {
  // The reply ends anyway once this tab stops reading if the server is
  // gone; nothing to report then.
  await api.conversations.stop(conversationId).catch(() => undefined);
  await entries.get(conversationId)?.chat.stop();
}


/**
 * Whether the newest user message was sent in research mode; the server
 * stores the flag with the message, so a chat opens as it was left.
 *
 * @param messages - Stored messages.
 * @returns True if research mode was on.
 */
export function lastResearch(messages: ChatMessage[]): boolean {
  return messages.findLast((message) => message.role === 'user')?.metadata?.research === true;
}

/**
 * The thinking effort the newest user message was sent with; the server
 * stores it with the message, so a chat opens as it was left.
 *
 * @param messages - Stored messages.
 * @returns The level, or null for the provider's default.
 */
export function lastEffort(messages: ChatMessage[]): EffortLevel | null {
  return messages.findLast((message) => message.role === 'user')?.metadata?.effort ?? null;
}

/**
 * The answer length the newest user message was sent with; the server
 * stores it with the message, so a chat opens as it was left.
 *
 * @param messages - Stored messages.
 * @returns The length; medium for chats from before it existed.
 */
export function lastVerbosity(messages: ChatMessage[]): Verbosity {
  return messages.findLast((message) => message.role === 'user')?.metadata?.verbosity ?? DEFAULT_VERBOSITY;
}

/**
 * The tone the newest user message was sent with, so a chat opens as it
 * was left.
 *
 * @param messages - Stored messages.
 * @returns The tone; neutral for chats from before it existed.
 */
export function lastTone(messages: ChatMessage[]): Tone {
  return messages.findLast((message) => message.role === 'user')?.metadata?.tone ?? DEFAULT_TONE;
}

/**
 * Summarizes a chat now, on the user's request, and puts the stored summary
 * into the open chat.
 *
 * @param conversationId - Conversation id.
 * @param selection - Model that writes the summary.
 */
export async function compactChat(conversationId: string, selection: ModelSelection): Promise<void> {
  upsertMessage(conversationId, await api.conversations.compact(conversationId, selection));
}

/**
 * Stops and forgets a conversation's session, e.g. after deleting it.
 *
 * @param conversationId - Conversation id.
 */
export function disposeChat(conversationId: string): void {
  void entries.get(conversationId)?.chat.stop();
  entries.delete(conversationId);
}

/**
 * Stops and forgets all sessions, e.g. on sign-out.
 */
export function disposeAllChats(): void {
  for (const id of [...entries.keys()]) {
    disposeChat(id);
  }
}
