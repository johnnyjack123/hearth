import { z } from 'zod';
import type { WebFetchAnswer } from './web-fetch.ts';

/**
 * Version of the device tunnel protocol. Server and device refuse to talk if
 * they disagree, instead of misreading each other's messages.
 */
export const TUNNEL_VERSION = 1;

/** Path of the device tunnel (WebSocket, device token as bearer). */
export const TUNNEL_PATH = '/api/device/tunnel';

/** Project names as harness allows them (`ProjectName` in harness-core). */
export const projectNameSchema = z.string().regex(/^[a-z0-9][a-z0-9-]{0,39}$/);

/** Harness names as harness allows them (`validate_harness_name`). */
export const harnessNameSchema = z.string().regex(/^[a-z0-9][a-z0-9-]{0,29}$/);

/**
 * A project a device offers to web chats: only projects the user released
 * with `harness project remote on`, and only harnesses with a machine
 * interface (adapter).
 */
export const remoteProjectSchema = z
  .object({
    name: projectNameSchema,
    harnesses: z.array(harnessNameSchema).max(20),
  })
  .strict();

/** See {@link remoteProjectSchema}. */
export type RemoteProject = z.infer<typeof remoteProjectSchema>;

const partId = z.string().min(1).max(100);
const text = (max: number) => z.string().max(max);

/** Longest tool output a device sends; it truncates longer ones itself. */
export const MAX_TOOL_OUTPUT = 64_000;

/**
 * What an agent does, as the device reports it: harness-independent, so the
 * server and the web app never see pi's (or any other harness's) own format.
 */
export const agentEventSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('turn.start') }).strict(),
  z.object({ type: z.literal('turn.end') }).strict(),
  z.object({ type: z.literal('text.start'), id: partId }).strict(),
  z.object({ type: z.literal('text.delta'), id: partId, delta: text(64_000) }).strict(),
  z.object({ type: z.literal('text.end'), id: partId }).strict(),
  z.object({ type: z.literal('reasoning.start'), id: partId }).strict(),
  z.object({ type: z.literal('reasoning.delta'), id: partId, delta: text(64_000) }).strict(),
  z.object({ type: z.literal('reasoning.end'), id: partId }).strict(),
  z
    .object({
      type: z.literal('tool.start'),
      id: partId,
      name: z.string().min(1).max(100),
      input: z.record(z.string(), z.unknown()),
    })
    .strict(),
  z.object({ type: z.literal('tool.update'), id: partId, output: text(MAX_TOOL_OUTPUT) }).strict(),
  z
    .object({ type: z.literal('tool.end'), id: partId, output: text(MAX_TOOL_OUTPUT), isError: z.boolean() })
    .strict(),
  z
    .object({
      type: z.literal('progress'),
      id: partId,
      label: text(200),
      state: z.enum(['running', 'done', 'failed']),
      detail: text(1_000).optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('notice'),
      level: z.enum(['info', 'warning', 'error']),
      text: text(4_000),
    })
    .strict(),
  z
    .object({
      type: z.literal('approval'),
      id: z.number().int().nonnegative(),
      host: z.string().min(1).max(255),
      port: z.number().int().min(1).max(65_535),
      state: z.enum(['pending', 'resolved']),
    })
    .strict(),
  z
    .object({
      type: z.literal('usage'),
      inputTokens: z.number().int().nonnegative(),
      outputTokens: z.number().int().nonnegative(),
      cachedInputTokens: z.number().int().nonnegative().optional(),
      cost: z.number().nonnegative().optional(),
      contextWindow: z.number().int().positive().optional(),
    })
    .strict(),
  z.object({ type: z.literal('error'), message: text(4_000) }).strict(),
  z.object({ type: z.literal('done'), aborted: z.boolean() }).strict(),
]);

/** See {@link agentEventSchema}. */
export type AgentEvent = z.infer<typeof agentEventSchema>;

const sessionId = z.string().min(1).max(100);

/** State of a device's VM: it starts when an agent needs it. */
export const VM_STATES = ['running', 'stopped', 'starting', 'unknown'] as const;

/** See {@link VM_STATES}. */
export type VmState = (typeof VM_STATES)[number];

/**
 * Validates the projects a device reports through the API (`harness init`),
 * so they appear in the account even while it is offline.
 */
export const deviceProjectsSchema = z
  .object({
    projects: z.array(remoteProjectSchema).max(200),
  })
  .strict();

/**
 * Messages a device sends through the tunnel. The server validates each one;
 * a device can only report on sessions the server opened with it.
 */
export const deviceMessageSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('hello'),
      version: z.number().int(),
      projects: z.array(remoteProjectSchema).max(200),
    })
    .strict(),
  z.object({ type: z.literal('projects'), projects: z.array(remoteProjectSchema).max(200) }).strict(),
  z.object({ type: z.literal('status'), vm: z.enum(VM_STATES) }).strict(),
  z.object({ type: z.literal('session.event'), session: sessionId, event: agentEventSchema }).strict(),
  z.object({ type: z.literal('session.closed'), session: sessionId, reason: text(1_000).optional() }).strict(),
]);

/** See {@link deviceMessageSchema}. */
export type DeviceMessage = z.infer<typeof deviceMessageSchema>;

/**
 * How the user answers a question of a project's gateway (a host that is not
 * allowed yet): the same choices as in `harness net watch`.
 */
export const APPROVAL_DECISIONS = ['once', 'minutes', 'session', 'always', 'deny'] as const;

/** See {@link APPROVAL_DECISIONS}. */
export type ApprovalDecision = (typeof APPROVAL_DECISIONS)[number];

/**
 * Messages the server sends to a device. The device checks each against its
 * own configuration (released projects, confirmed chats) before acting.
 */
export type ServerMessage =
  | {
      type: 'welcome';
      /** Protocol version of the server. */
      version: number;
    }
  | {
      type: 'session.prompt';
      /** Session id; the conversation id, so each chat keeps one agent session. */
      session: string;
      /** Project to work in. */
      project: string;
      /** Harness to run. */
      harness: string;
      /** The chat, so the device can name it when it asks the user. */
      conversation: { id: string; title: string | null };
      /** The user's message. */
      text: string;
      /** Earlier messages as text; used only when the agent session is new. */
      history: string | null;
      /** Model chosen in the chat; the device maps it to the harness's provider. */
      model: { credentialId: string; modelId: string } | null;
    }
  | {
      type: 'session.cancel';
      /** Session to interrupt. */
      session: string;
    }
  | {
      type: 'session.close';
      /** Session to end; the harness process stops. */
      session: string;
    }
  | {
      type: 'approval.decide';
      /** Session whose gateway asked. */
      session: string;
      /** Question id from the `approval` event. */
      id: number;
      /** The answer. */
      decision: ApprovalDecision;
    };

/**
 * Where a conversation's agent runs.
 */
export interface ChatLink {
  /** Device the agent runs on. */
  deviceId: string;
  /** The device's name. */
  deviceName: string;
  /** Project on that device. */
  project: string;
  /** Harness, e.g. `pi`. */
  harness: string;
}

/**
 * Validates a request that links a conversation to a project on a device.
 */
export const linkConversationSchema = z
  .object({
    deviceId: z.string().min(1).max(100),
    project: projectNameSchema,
    harness: harnessNameSchema,
  })
  .strict();

/** See {@link linkConversationSchema}. */
export type LinkConversationInput = z.infer<typeof linkConversationSchema>;

/**
 * Validates the answer to a gateway question sent from the chat.
 */
export const approvalDecisionSchema = z
  .object({
    decision: z.enum(APPROVAL_DECISIONS),
  })
  .strict();

/**
 * Data parts of agent replies, beyond the AI SDK's own text, reasoning and
 * tool parts. A type alias, not an interface: the AI SDK requires it to be
 * assignable to a record, which interfaces are not.
 */
export type AgentDataParts = {
  /**
   * A message of harness or the harness itself (warnings, progress). Lines
   * after the first are details the web app shows on demand.
   */
  notice: { level: 'info' | 'warning' | 'error'; text: string };
  /**
   * A question of the project's gateway, or of the server's web fetch in an
   * ordinary chat (then with `url`); updated in place when answered.
   */
  approval: {
    id: number;
    host: string;
    port: number;
    state: 'pending' | 'resolved';
    /** Full URL a web fetch wants to load; only set for web fetch questions. */
    url?: string;
    /** How a web fetch question ended; set once it is resolved. */
    answer?: WebFetchAnswer;
  };
  /** A step on the way to the answer; updated in place. */
  progress: { id: string; label: string; state: 'running' | 'done' | 'failed'; detail?: string };
};

/**
 * Output of a tool call in an agent reply.
 */
export interface AgentToolOutput {
  /** What the tool printed or returned, possibly truncated by the device. */
  text: string;
}
