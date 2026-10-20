import type { AgentEvent, ChatLink, MessageMetadata } from '@harness/shared';
import { createUIMessageStream, createUIMessageStreamResponse, type InferUIMessageChunk } from 'ai';
import type { Kysely } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import type { DB } from '../db/schema.ts';
import { type ChatMessage, saveMessage, updateConversation } from '../services/conversations.ts';
import type { AgentRun } from '../tunnel/hub.ts';

type Chunk = InferUIMessageChunk<ChatMessage>;

/** Earlier messages passed to a new agent session, newest kept. */
const MAX_HISTORY_CHARS = 20_000;

/**
 * Turns earlier messages into plain text for an agent that joins a running
 * chat. Only text parts count; tool calls of an earlier agent stay out.
 *
 * @param messages - Messages before the new one.
 * @returns The transcript, or null if there is nothing to pass on.
 */
export function transcript(messages: ChatMessage[]): string | null {
  const lines = messages.flatMap((message) => {
    const text = message.parts
      .map((part) => (part.type === 'text' ? part.text : ''))
      .join('')
      .trim();
    if (!text || (message.role !== 'user' && message.role !== 'assistant')) {
      return [];
    }
    return [`${message.role === 'user' ? 'User' : 'Assistant'}: ${text}`];
  });
  if (lines.length === 0) {
    return null;
  }
  const all = lines.join('\n\n');
  return all.length <= MAX_HISTORY_CHARS ? all : `…${all.slice(-MAX_HISTORY_CHARS)}`;
}

/**
 * Translates agent events into UI message chunks. Keeps track of open tool
 * calls, because the AI SDK rejects output for a call it has not seen.
 */
export class ChunkMapper {
  readonly #tools = new Set<string>();
  #usage: NonNullable<MessageMetadata['usage']> | undefined;

  /**
   * Usage reported so far: tokens and cost summed over the agent's model
   * calls, the context as after the last one.
   */
  get usage(): NonNullable<MessageMetadata['usage']> | undefined {
    return this.#usage;
  }

  /**
   * @param event - Event from the device.
   * @returns The chunks to write, possibly none.
   */
  map(event: AgentEvent): Chunk[] {
    switch (event.type) {
      case 'turn.start':
        return [{ type: 'start-step' }];
      case 'turn.end':
        return [{ type: 'finish-step' }];
      case 'text.start':
        return [{ type: 'text-start', id: event.id }];
      case 'text.delta':
        return [{ type: 'text-delta', id: event.id, delta: event.delta }];
      case 'text.end':
        return [{ type: 'text-end', id: event.id }];
      case 'reasoning.start':
        return [{ type: 'reasoning-start', id: event.id }];
      case 'reasoning.delta':
        return [{ type: 'reasoning-delta', id: event.id, delta: event.delta }];
      case 'reasoning.end':
        return [{ type: 'reasoning-end', id: event.id }];
      case 'tool.start':
        this.#tools.add(event.id);
        return [
          { type: 'tool-input-available', toolCallId: event.id, toolName: event.name, input: event.input, dynamic: true },
        ];
      case 'tool.update':
        return this.#tools.has(event.id)
          ? [
              {
                type: 'tool-output-available',
                toolCallId: event.id,
                output: { text: event.output },
                dynamic: true,
                preliminary: true,
              },
            ]
          : [];
      case 'tool.end':
        if (!this.#tools.delete(event.id)) {
          return [];
        }
        return event.isError
          ? [{ type: 'tool-output-error', toolCallId: event.id, errorText: event.output || 'Error', dynamic: true }]
          : [{ type: 'tool-output-available', toolCallId: event.id, output: { text: event.output }, dynamic: true }];
      case 'progress':
        // Same id, so a finished step replaces the running one in place.
        return [
          {
            type: 'data-progress',
            id: `progress-${event.id}`,
            data: {
              id: event.id,
              label: event.label,
              state: event.state,
              ...(event.detail === undefined ? {} : { detail: event.detail }),
            },
          },
        ];
      case 'notice':
        return [{ type: 'data-notice', data: { level: event.level, text: event.text } }];
      case 'approval':
        // Same id, so the answered question replaces the open one in place.
        return [
          {
            type: 'data-approval',
            id: `approval-${event.id}`,
            data: { id: event.id, host: event.host, port: event.port, state: event.state },
          },
        ];
      case 'usage': {
        const before = this.#usage;
        const next: NonNullable<MessageMetadata['usage']> = {
          inputTokens: (before?.inputTokens ?? 0) + event.inputTokens,
          outputTokens: (before?.outputTokens ?? 0) + event.outputTokens,
          contextTokens: event.inputTokens + event.outputTokens,
        };
        const cached = (before?.cachedInputTokens ?? 0) + (event.cachedInputTokens ?? 0);
        if (cached > 0) {
          next.cachedInputTokens = cached;
        }
        const window = event.contextWindow ?? before?.contextWindow;
        if (window !== undefined) {
          next.contextWindow = window;
        }
        if (event.cost !== undefined || before?.cost !== undefined) {
          next.cost = (before?.cost ?? 0) + (event.cost ?? 0);
        }
        // pi reports each model call with its own total; tool names stay
        // unknown here, the tool blocks of the reply show them.
        next.calls = [
          ...(before?.calls ?? []),
          {
            input: event.inputTokens,
            cacheRead: event.cachedInputTokens ?? 0,
            cacheWrite: 0,
            output: event.outputTokens,
            tools: [],
            ...(event.cost !== undefined ? { cost: { total: event.cost } } : {}),
          },
        ];
        this.#usage = next;
        return [];
      }
      case 'error':
        return [{ type: 'error', errorText: event.message }];
      case 'done':
        return [];
    }
  }

  /**
   * Chunks that close tool calls the agent never finished (abort, lost
   * device), so the chat does not show them as running forever.
   *
   * @returns Error chunks for the open calls.
   */
  closeOpenTools(): Chunk[] {
    const chunks: Chunk[] = [...this.#tools].map((id) => ({
      type: 'tool-output-error' as const,
      toolCallId: id,
      errorText: 'interrupted',
      dynamic: true,
    }));
    this.#tools.clear();
    return chunks;
  }
}

/**
 * Options for {@link streamAgent}.
 */
export interface StreamAgentOptions {
  /** Open database. */
  db: Kysely<DB>;
  /** Conversation being continued, already checked for ownership. */
  conversationId: string;
  /** Where the agent runs. */
  link: ChatLink;
  /** Stored history plus the new user message. */
  messages: ChatMessage[];
  /** The running reply. */
  run: AgentRun;
  /** Model chosen in the chat, stored in the reply's metadata. */
  selection: { credentialId: string; modelId: string };
  /** Aborted when the client presses stop or disconnects. */
  abortSignal: AbortSignal;
}

/**
 * Streams an agent's reply in the AI SDK UI message stream format, so the
 * chat renders it like a model reply (with tool calls), and stores it when
 * the agent is done.
 *
 * @param options - See {@link StreamAgentOptions}.
 * @returns A streaming HTTP response.
 */
export function streamAgent(options: StreamAgentOptions): Response {
  const { db, conversationId, link, messages, run, selection, abortSignal } = options;
  const metadata: MessageMetadata = {
    createdAt: Date.now(),
    modelId: selection.modelId,
    credentialId: selection.credentialId,
    agent: { project: link.project, harness: link.harness },
  };
  const mapper = new ChunkMapper();
  // Stop in the chat interrupts the agent; it then reports `done`.
  abortSignal.addEventListener('abort', () => run.cancel(), { once: true });

  const stream = createUIMessageStream<ChatMessage>({
    originalMessages: messages,
    generateId: uuidv7,
    execute: async ({ writer }) => {
      writer.write({ type: 'start', messageMetadata: metadata });
      for await (const event of run) {
        for (const chunk of mapper.map(event)) {
          writer.write(chunk);
        }
      }
      for (const chunk of mapper.closeOpenTools()) {
        writer.write(chunk);
      }
      writer.write({ type: 'finish', messageMetadata: { ...metadata, ...(mapper.usage ? { usage: mapper.usage } : {}) } });
    },
    onError: (error) => {
      console.error('[agent] stream failed', error);
      return 'The agent reply could not be streamed.';
    },
    onEnd: async ({ responseMessage }) => {
      if (responseMessage.parts.length === 0) {
        return;
      }
      await saveMessage(db, conversationId, {
        ...responseMessage,
        metadata: { ...metadata, ...responseMessage.metadata },
      });
      await updateConversation(db, conversationId, {});
    },
  });
  return createUIMessageStreamResponse({ stream });
}
