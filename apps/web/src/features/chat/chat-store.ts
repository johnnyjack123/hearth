import { Chat } from '@ai-sdk/react';
import type { ConversationDetail, ModelSelection } from '@harness/shared';
import { DefaultChatTransport } from 'ai';
import type { ChatMessage } from '@/lib/api.ts';
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
 * Returns the chat session of a conversation, creating it on first use.
 *
 * @param conversationId - Conversation id; also the chat id.
 * @param initialMessages - Stored messages, used only when the session is created.
 * @param initialSelection - Model for the first request, used only when the session is created.
 * @returns The session entry.
 */
export function getChat(
  conversationId: string,
  initialMessages: ChatMessage[],
  initialSelection: ModelSelection | null,
): ChatEntry {
  const existing = entries.get(conversationId);
  if (existing) {
    return existing;
  }

  const selection = { current: initialSelection };
  const chat: Chat<ChatMessage> = new Chat<ChatMessage>({
    id: conversationId,
    messages: initialMessages,
    transport: new DefaultChatTransport<ChatMessage>({
      api: `/api/conversations/${encodeURIComponent(conversationId)}/chat`,
      // The server owns the history; only the newest message is sent.
      prepareSendMessagesRequest: ({ messages }) => ({
        body: {
          message: messages.at(-1),
          credentialId: selection.current?.credentialId,
          modelId: selection.current?.modelId,
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

  const entry = { chat, selection };
  entries.set(conversationId, entry);
  return entry;
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
