import type { AgentEvent, AgentToolOutput, ChatLink, Compaction, MessageMetadata, ModelCallUsage } from '@harness/shared';
import { createUIMessageStream, createUIMessageStreamResponse, type InferUIMessageChunk } from 'ai';
import type { Kysely } from 'kysely';
import { v7 as uuidv7 } from 'uuid';
import type { DB } from '../db/schema.ts';
import { type ChatMessage, saveMessage, updateConversation } from '../services/conversations.ts';
import type { AgentRun } from '../tunnel/hub.ts';
import { agentWarning, type Assessor } from '../assess.ts';
import type { Helpers } from '../helpers/openrouter.ts';
import { type AgentCallTokens, withHelpers } from '../pricing.ts';

type Chunk = InferUIMessageChunk<ChatMessage>;

/**
 * Prices one model call of an agent (see `agentCallCost`); without one,
 * pi's own figure counts.
 */
export type AgentPricer = (call: AgentCallTokens) => Pick<ModelCallUsage, 'cost' | 'billed' | 'host' | 'rates'>;

/**
 * The cost fields of a call as pi reports it, through `price` if given.
 *
 * @param call - The call's tokens and pi's figure.
 * @param price - The server's pricing, if any.
 * @returns `cost` and `billed` for the call's entry.
 */
function priceCall(call: AgentCallTokens, price: AgentPricer | undefined): Pick<ModelCallUsage, 'cost' | 'billed' | 'host' | 'rates'> {
  if (price) {
    return price(call);
  }
  return call.agentCost === undefined ? {} : { cost: { total: call.agentCost } };
}

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
  // Open tool calls with their name, arguments and start.
  readonly #tools = new Map<string, { name: string; input: unknown; started: number }>();
  // How long each finished call ran.
  readonly #toolMs: Record<string, number> = {};
  // Summaries of the agent in this reply; each one updates in place.
  #compactions = 0;
  #compacting = false;
  #usage: NonNullable<MessageMetadata['usage']> | undefined;
  // When the agent's current model call began, and when its first token
  // came; between that and its usage the model wrote (tokens per second).
  #callStarted: number | undefined;
  #firstToken: number | undefined;

  /**
   * @param model - Model chosen in the chat; pi writes its summaries with it.
   * @param now - The clock, for tests.
   * @param price - Prices the agent's model calls; pi's figures without it.
   */
  constructor(
    readonly model?: string,
    private readonly now: () => number = Date.now,
    private readonly price?: AgentPricer,
  ) {}

  /**
   * Usage reported so far: tokens and cost summed over the agent's model
   * calls, the context as after the last one.
   */
  get usage(): NonNullable<MessageMetadata['usage']> | undefined {
    return this.#usage;
  }

  /** How long each finished tool call of the reply ran, by id. */
  get toolMs(): Record<string, number> {
    return { ...this.#toolMs };
  }

  /**
   * @param id - Id of an open tool call.
   * @returns Its tool's name, if the call is open.
   */
  toolName(id: string): string | undefined {
    return this.#tools.get(id)?.name;
  }

  /**
   * @param id - Id of an open tool call.
   * @returns Its arguments, if the call is open.
   */
  toolInput(id: string): unknown {
    return this.#tools.get(id)?.input;
  }

  /**
   * @param event - Event from the device.
   * @param extra - More for the output of a finished tool call: the
   *   server's findings and Jev's opinion.
   * @returns The chunks to write, possibly none.
   */
  map(event: AgentEvent, extra?: Partial<AgentToolOutput>): Chunk[] {
    switch (event.type) {
      case 'turn.start':
        this.#callStarted = this.now();
        this.#firstToken = undefined;
        return [{ type: 'start-step' }];
      case 'turn.end':
        return [{ type: 'finish-step' }];
      case 'text.start':
        return [{ type: 'text-start', id: event.id }];
      case 'text.delta':
        this.#firstToken ??= this.now();
        return [{ type: 'text-delta', id: event.id, delta: event.delta }];
      case 'text.end':
        return [{ type: 'text-end', id: event.id }];
      case 'reasoning.start':
        return [{ type: 'reasoning-start', id: event.id }];
      case 'reasoning.delta':
        this.#firstToken ??= this.now();
        return [{ type: 'reasoning-delta', id: event.id, delta: event.delta }];
      case 'reasoning.end':
        return [{ type: 'reasoning-end', id: event.id }];
      case 'tool.start':
        this.#tools.set(event.id, { name: event.name, input: event.input, started: this.now() });
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
      case 'tool.end': {
        const open = this.#tools.get(event.id);
        if (!open) {
          return [];
        }
        this.#tools.delete(event.id);
        this.#toolMs[event.id] = Math.max(0, this.now() - open.started);
        return event.isError
          ? [{ type: 'tool-output-error', toolCallId: event.id, errorText: event.output || 'Error', dynamic: true }]
          : [{ type: 'tool-output-available', toolCallId: event.id, output: { ...extra, text: event.output }, dynamic: true }];
      }
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
        if (before?.helpers) {
          next.helpers = before.helpers;
        }
        // pi reports each model call with its own total; the server's price
        // replaces it where it knows better (what OpenRouter billed,
        // DeepSeek's hours).
        const priced = priceCall(
          {
            input: event.inputTokens,
            cacheRead: event.cachedInputTokens ?? 0,
            output: event.outputTokens,
            ...(event.cost !== undefined ? { agentCost: event.cost } : {}),
          },
          this.price,
        );
        if (priced.cost !== undefined || before?.cost !== undefined) {
          next.cost = (before?.cost ?? 0) + (priced.cost?.total ?? 0);
        }
        // Tool names stay unknown here, the tool blocks of the reply show them.
        const ended = this.now();
        const ms = ended - (this.#firstToken ?? this.#callStarted ?? ended);
        next.calls = [
          ...(before?.calls ?? []),
          {
            input: event.inputTokens,
            cacheRead: event.cachedInputTokens ?? 0,
            cacheWrite: 0,
            output: event.outputTokens,
            tools: [],
            ...priced,
            ...(ms > 0 ? { ms } : {}),
          },
        ];
        if (next.calls.every((call) => call.billed)) {
          next.billed = true;
        }
        // A turn may hold more calls; the next one starts now.
        this.#callStarted = ended;
        this.#firstToken = undefined;
        this.#usage = next;
        return [];
      }
      case 'takeover':
        // One report per reply, replaced in place as the takeover goes on.
        return [{ type: 'data-takeover', id: 'takeover', data: event.report }];
      case 'workspace':
        return [{ type: 'data-workspace', id: 'workspace', data: { commits: event.commits, changes: event.changes } }];
      case 'compaction': {
        // A finished one without a start (an older pi) gets its own place.
        if (event.state === 'running' || !this.#compacting) {
          this.#compactions += 1;
        }
        this.#compacting = event.state === 'running';
        // Writing the summary is a model call of its own; it costs like one.
        const call = event.state === 'done' ? compactionCall(event, this.model, this.price) : null;
        if (call) {
          this.#usage = withCall(this.#usage, call);
        }
        return [{ type: 'data-compaction', id: `compaction-agent-${this.#compactions}`, data: agentCompaction(event) }];
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
    const chunks: Chunk[] = [...this.#tools.keys()].map((id) => ({
      type: 'tool-output-error' as const,
      toolCallId: id,
      errorText: 'interrupted',
      dynamic: true,
    }));
    this.#tools.clear();
    return chunks;
  }
}

/** A `compaction` event of an agent. */
export type CompactionEvent = Extract<AgentEvent, { type: 'compaction' }>;

/**
 * An agent's summary as the chat stores it.
 *
 * @param event - pi's report.
 * @returns The `data-compaction` part's data.
 */
export function agentCompaction(event: CompactionEvent): Compaction {
  return {
    state: event.state,
    trigger: event.trigger,
    agent: true,
    ...(event.summary ? { summary: event.summary } : {}),
    ...(event.tokensBefore !== undefined ? { tokens: event.tokensBefore } : {}),
    ...(event.tokensAfter !== undefined ? { tokensAfter: event.tokensAfter } : {}),
    ...(event.cost !== undefined ? { cost: event.cost } : {}),
    ...(event.error ? { error: event.error } : {}),
  };
}

/**
 * The model call that wrote an agent's summary, as a helper call of the
 * reply.
 *
 * @param event - pi's report of a finished summary.
 * @param model - Model chosen in the chat, if known.
 * @param price - The server's pricing of agent calls, if any.
 * @returns The call, or null if pi said nothing about tokens or cost.
 */
export function compactionCall(event: CompactionEvent, model: string | undefined, price?: AgentPricer): ModelCallUsage | null {
  if (event.cost === undefined && event.inputTokens === undefined && event.outputTokens === undefined) {
    return null;
  }
  const input = event.inputTokens ?? 0;
  const output = event.outputTokens ?? 0;
  return {
    input,
    cacheRead: 0,
    cacheWrite: 0,
    output,
    tools: [],
    ...(model ? { model } : {}),
    purpose: 'compact',
    ...priceCall({ input, cacheRead: 0, output, ...(event.cost !== undefined ? { agentCost: event.cost } : {}) }, price),
  };
}

/**
 * Adds a helper call to a reply's usage, which may not have any yet.
 *
 * @param usage - The reply's usage so far.
 * @param call - The call.
 * @returns The usage with the call and its cost.
 */
export function withCall(usage: NonNullable<MessageMetadata['usage']> | undefined, call: ModelCallUsage): NonNullable<MessageMetadata['usage']> {
  return usage ? withHelpers(usage, [call]) : { helpers: [call], ...(call.cost ? { cost: call.cost.total } : {}) };
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
  /** Aborted when the user presses stop; a disconnect does not. */
  abortSignal: AbortSignal;
  /** Gets a copy of the SSE stream, which keeps the reply going without the page. */
  consumeSseStream?: (stream: ReadableStream<string>) => void;
  /**
   * Asks Jev about tool output that looks like a prompt injection, before it
   * is shown; null unless the user wants that. Its cost joins the reply.
   */
  assess?: { assessor: Assessor; helpers: Helpers } | null;
  /** Prices the agent's model calls; pi's own figures without it. */
  price?: AgentPricer;
  /**
   * Hears the reply's cost after each model call, and when it is stored:
   * the daily cost limit counts a running reply, too.
   */
  costs?: { report: (dollars: number) => void; end: () => void };
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
  const mapper = new ChunkMapper(selection.modelId, Date.now, options.price);
  // The server checks tool output itself: the device's guard only reports
  // a notice, without the places.
  async function toolExtras(event: AgentEvent): Promise<Partial<AgentToolOutput> | undefined> {
    if (event.type !== 'tool.end' || event.isError) {
      return undefined;
    }
    const warned = agentWarning(mapper.toolName(event.id) ?? 'tool', mapper.toolInput(event.id), event.output);
    if (!warned) {
      return undefined;
    }
    const assessment = options.assess ? await options.assess.assessor(warned) : null;
    return { findings: warned.findings, ...(assessment ? { assessment } : {}) };
  }
  // Stop in the chat interrupts the agent; it then reports `done`.
  abortSignal.addEventListener('abort', () => run.cancel(), { once: true });

  const stream = createUIMessageStream<ChatMessage>({
    originalMessages: messages,
    generateId: uuidv7,
    execute: async ({ writer }) => {
      writer.write({ type: 'start', messageMetadata: metadata });
      const usage = () => (mapper.usage && options.assess ? withHelpers(mapper.usage, options.assess.helpers.usage) : mapper.usage);
      const current = (): MessageMetadata => {
        const now = usage();
        const toolMs = mapper.toolMs;
        return { ...metadata, ...(now ? { usage: now } : {}), ...(Object.keys(toolMs).length > 0 ? { toolMs } : {}) };
      };
      for await (const event of run) {
        for (const chunk of mapper.map(event, await toolExtras(event))) {
          writer.write(chunk);
        }
        // Cost and context after every model call, not only at the end: a
        // long reply shows them as it goes, and one that breaks off (the
        // agent killed, the device gone) keeps what it used. Likewise how
        // long each tool ran.
        if (event.type === 'usage' || event.type === 'compaction' || event.type === 'tool.end') {
          const now = current();
          writer.write({ type: 'message-metadata', messageMetadata: now });
          if (now.usage?.cost !== undefined) {
            options.costs?.report(now.usage.cost);
          }
        }
      }
      for (const chunk of mapper.closeOpenTools()) {
        writer.write(chunk);
      }
      writer.write({ type: 'finish', messageMetadata: current() });
    },
    onError: (error) => {
      console.error('[agent] stream failed', error);
      return 'The agent reply could not be streamed.';
    },
    onEnd: async ({ responseMessage }) => {
      if (responseMessage.parts.length === 0) {
        return;
      }
      try {
        await saveMessage(db, conversationId, {
          ...responseMessage,
          metadata: { ...metadata, ...responseMessage.metadata },
        });
      } finally {
        // Stored, its cost counts in `costDay`; or lost, and then not at all.
        options.costs?.end();
      }
      await updateConversation(db, conversationId, {});
    },
  });
  return createUIMessageStreamResponse({
    stream,
    ...(options.consumeSseStream ? { consumeSseStream: ({ stream: copy }) => options.consumeSseStream?.(copy) } : {}),
  });
}
