import { z } from 'zod';
import type { EffortLevel } from './api.ts';
import type { DeviceAppTemplate } from './app-templates.ts';
import type { ChatFile } from './files.ts';
import type { InjectionAssessment, InjectionFinding, WebFetchAnswer } from './web-fetch.ts';

/**
 * Version of the device tunnel protocol. The server greets with its version,
 * the device answers with the lower of both, and each side only sends what
 * that version knows. So server and device can be updated one after the
 * other.
 *
 * Version 2: VM activity in `status`, the idle time (`settings`) and
 * stopping the VM from the browser (`vm.stop`, `vm.result`).
 * Version 3: taking over the agent's work into the user's folder.
 * Version 4: pi's settings from the chat: the thinking level with each
 * prompt, summarizing on request (`session.compact`) and the `compaction`
 * event for pi's own summaries.
 * Version 5: the app catalog: the device reports its apps (`apps`), the
 * browser switches them per project (`apps.set`, `apps.result`).
 * Version 6: the account's own programs (`apps.templates`), and where each
 * app of a report comes from (`source`).
 * Version 7: program files from the computer: the browser asks
 * (`apps.file`), the device answers with the id of a request the user
 * approves on the computer (`approval` in `apps.result`) and reports the
 * requests still waiting (`pending`).
 * Version 8: after each reply and takeover the device says what of the
 * agent's work the user's folder lacks (`workspace` event).
 * Version 9: prompts carry the files attached in the chat (`files`); the
 * device downloads them from the server and copies them into the instance.
 */
export const TUNNEL_VERSION = 10;

/** Oldest device protocol the server still accepts. */
export const MIN_TUNNEL_VERSION = 1;

/** First protocol with VM activity, idle time and `vm.stop`. */
export const VERSION_VM_CONTROL = 2;

/**
 * First protocol that takes over the agent's work: `session.takeover`, the
 * `takeover` event and `autosync` in prompts.
 */
export const VERSION_TAKEOVER = 3;

/**
 * First protocol with pi's settings from the chat: `thinking` in prompts,
 * `session.compact` and the `compaction` event.
 */
export const VERSION_PI_SETTINGS = 4;

/**
 * First protocol with the app catalog: `apps` from the device, `apps.set`
 * and its `apps.result`.
 */
export const VERSION_APPS = 5;

/**
 * First protocol with the account's own programs: `apps.templates` to the
 * device, `source` in its app reports.
 */
export const VERSION_TEMPLATES = 6;

/**
 * First protocol with program files from the computer: `apps.file`,
 * `approval` in `apps.result` and `pending` in app reports.
 */
export const VERSION_APP_FILES = 7;

/** First protocol with the `workspace` event. */
export const VERSION_WORKSPACE = 8;

/** First protocol with files in prompts. */
export const VERSION_FILES = 9;

/** First protocol with program versions: the server's in `welcome`, hearth's in `hello`. */
export const VERSION_APP_VERSION = 10;

/** Ids of requests the user approves on the computer (`hearth app approve`). */
export const approvalIdSchema = z.string().regex(/^[A-HJ-NP-Z2-9]{6}$/);

/** Idle minutes until a device stops its VM, unless the user chose otherwise. */
export const DEFAULT_IDLE_MINUTES = 30;

/** Longest idle time one can set; 0 means the VM never stops by itself. */
export const MAX_IDLE_MINUTES = 24 * 60;

/** Path of the device tunnel (WebSocket, device token as bearer). */
export const TUNNEL_PATH = '/api/device/tunnel';

/** Project names as hearth allows them (`ProjectName` in hearth-core). */
export const projectNameSchema = z.string().regex(/^[a-z0-9][a-z0-9-]{0,39}$/);

/** Harness names as hearth allows them (`validate_harness_name`). */
export const harnessNameSchema = z.string().regex(/^[a-z0-9][a-z0-9-]{0,29}$/);

/**
 * A project a device offers to web chats: only projects the user released
 * with `hearth project remote on`, and only harnesses with a machine
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

/** App names as hearth allows them (`validate_app_name`, same rules as harnesses). */
export const appNameSchema = harnessNameSchema;

/**
 * Where an app of a device comes from: the catalog shipped with hearth, a
 * recipe file on that computer, or an own program of the account.
 */
export const APP_SOURCES = ['builtin', 'local', 'server'] as const;

/** See {@link APP_SOURCES}. */
export type AppSource = (typeof APP_SOURCES)[number];

/**
 * A program a device can give its agents (app catalog): built in or a recipe
 * of the user's on that computer.
 */
export const appInfoSchema = z
  .object({
    name: appNameSchema,
    title: z.string().min(1).max(60),
    description: z.string().max(300).optional(),
    /** Apps it brings along when switched on. */
    requires: z.array(appNameSchema).max(20),
    /** Version installed in the device's VM; null if not installed or unknown. */
    version: z.string().max(40).nullable(),
    /** Whether its recipe changed since it was installed. */
    outdated: z.boolean(),
    /** Where it comes from; protocol 6. */
    source: z.enum(APP_SOURCES).optional(),
  })
  .strict();

/** See {@link appInfoSchema}. */
export type AppInfo = z.infer<typeof appInfoSchema>;

/**
 * A program file the browser asked for, waiting on the computer for the
 * user's approval.
 */
export const pendingFileSchema = z
  .object({
    id: approvalIdSchema,
    name: appNameSchema,
    title: z.string().min(1).max(60),
    path: z.string().min(1).max(1_000),
    kind: z.string().min(1).max(100),
  })
  .strict();

/** See {@link pendingFileSchema}. */
export type PendingFile = z.infer<typeof pendingFileSchema>;

/** The apps a project switched on, without the ones they require. */
export const projectAppsSchema = z
  .object({
    name: projectNameSchema,
    enabled: z.array(appNameSchema).max(100),
  })
  .strict();

/** See {@link projectAppsSchema}. */
export type ProjectApps = z.infer<typeof projectAppsSchema>;

/**
 * A device's apps: what it knows, what its offered projects switched on,
 * and whether the installed versions are known (only once its VM ran).
 */
export const appsReportSchema = z
  .object({
    catalog: z.array(appInfoSchema).max(200),
    versions: z.boolean(),
    projects: z.array(projectAppsSchema).max(200),
    /** Program files waiting for approval on the computer; protocol 7. */
    pending: z.array(pendingFileSchema).max(20).optional(),
  })
  .strict();

/** See {@link appsReportSchema}. */
export type AppsReport = z.infer<typeof appsReportSchema>;

const partId = z.string().min(1).max(100);
const text = (max: number) => z.string().max(max);

/** Longest tool output a device sends; it truncates longer ones itself. */
export const MAX_TOOL_OUTPUT = 64_000;

const count = z.number().int().nonnegative();

/** A finding of the merge check, as the device reports it (German text). */
export const reportedFindingSchema = z
  .object({
    severity: z.enum(['high', 'medium', 'low']),
    rule: z.string().min(1).max(100),
    path: text(1_000),
    line: count.optional(),
    message: text(2_000),
    excerpt: text(1_000).optional(),
  })
  .strict();

/** See {@link reportedFindingSchema}. */
export type ReportedFinding = z.infer<typeof reportedFindingSchema>;

/** How taking over the agent's work stands or ended. */
export const TAKEOVER_STATES = ['running', 'merged', 'up_to_date', 'held', 'conflict', 'refused', 'failed'] as const;

/**
 * Report of taking over the agent's work into the user's folder: commit in
 * the container, bring the commits home, check them, merge into the
 * checked-out branch. Severe findings hold the merge until the user
 * approves it in the terminal with `request`.
 */
export const takeoverReportSchema = z
  .object({
    state: z.enum(TAKEOVER_STATES),
    step: text(200).optional(),
    auto: z.boolean(),
    committed: count.optional(),
    commits: count.optional(),
    files: count.optional(),
    branch: text(300).optional(),
    checks: z.array(text(100)).max(10),
    findings: z.array(reportedFindingSchema).max(100),
    request: z
      .string()
      .regex(/^[A-Z2-9]{6}$/)
      .optional(),
    conflicts: z.array(text(1_000)).max(50).optional(),
    notes: z.array(text(4_000)).max(20).optional(),
    reason: text(4_000).optional(),
  })
  .strict();

/** See {@link takeoverReportSchema}. */
export type TakeoverReport = z.infer<typeof takeoverReportSchema>;

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
  z.object({ type: z.literal('takeover'), report: takeoverReportSchema }).strict(),
  z
    .object({
      type: z.literal('compaction'),
      state: z.enum(['running', 'done', 'failed']),
      trigger: z.enum(['auto', 'manual']),
      summary: text(100_000).optional(),
      tokensBefore: z.number().int().nonnegative().optional(),
      tokensAfter: z.number().int().nonnegative().optional(),
      inputTokens: z.number().int().nonnegative().optional(),
      outputTokens: z.number().int().nonnegative().optional(),
      cost: z.number().nonnegative().optional(),
      error: text(4_000).optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('workspace'),
      commits: z.number().int().nonnegative().max(1_000_000),
      changes: z.number().int().nonnegative().max(1_000_000),
    })
    .strict(),
  z.object({ type: z.literal('done'), aborted: z.boolean() }).strict(),
]);

/** See {@link agentEventSchema}. */
export type AgentEvent = z.infer<typeof agentEventSchema>;

const sessionId = z.string().min(1).max(100);

/**
 * State of a device's VM: it starts when an agent needs it. `stopping` comes
 * from protocol 2 on.
 */
export const VM_STATES = ['running', 'stopped', 'starting', 'stopping', 'unknown'] as const;

/** See {@link VM_STATES}. */
export type VmState = (typeof VM_STATES)[number];

/**
 * Validates the projects a device reports through the API (`hearth init`),
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
      // From protocol 10 on: the version of hearth on the computer.
      harness: z
        .string()
        .max(40)
        .regex(/^[\w.+-]+$/)
        .optional(),
    })
    .strict(),
  z.object({ type: z.literal('projects'), projects: z.array(remoteProjectSchema).max(200) }).strict(),
  z
    .object({
      type: z.literal('status'),
      vm: z.enum(VM_STATES),
      // From protocol 2 on: what keeps the VM busy, and in how many seconds
      // the device stops it when nothing happens (null: it will not).
      replies: z.number().int().nonnegative().max(1_000).optional(),
      terminals: z.number().int().nonnegative().max(1_000).optional(),
      stopIn: z.number().int().nonnegative().max(MAX_IDLE_MINUTES * 60).nullable().optional(),
    })
    .strict(),
  z.object({ type: z.literal('session.event'), session: sessionId, event: agentEventSchema }).strict(),
  z.object({ type: z.literal('session.closed'), session: sessionId, reason: text(1_000).optional() }).strict(),
  z
    .object({
      type: z.literal('vm.result'),
      request: z.string().min(1).max(100),
      ok: z.boolean(),
      message: text(1_000),
    })
    .strict(),
  z.object({ type: z.literal('apps'), report: appsReportSchema }).strict(),
  z
    .object({
      type: z.literal('apps.result'),
      request: z.string().min(1).max(100),
      ok: z.boolean(),
      message: text(1_000),
      // Protocol 7: the request to approve on the computer.
      approval: approvalIdSchema.optional(),
    })
    .strict(),
]);

/** See {@link deviceMessageSchema}. */
export type DeviceMessage = z.infer<typeof deviceMessageSchema>;

/**
 * How the user answers a question of a project's gateway (a host that is not
 * allowed yet): the same choices as in `hearth net watch`.
 */
export const APPROVAL_DECISIONS = ['once', 'minutes', 'session', 'always', 'deny'] as const;

/** See {@link APPROVAL_DECISIONS}. */
export type ApprovalDecision = (typeof APPROVAL_DECISIONS)[number];

/**
 * What the server's model catalog says about the model of a linked chat.
 * pi only lets a chat pick models it knows (31.10.: OpenRouter's
 * `deepseek/deepseek-v4.1-flash-1017` was missing, and pi silently kept
 * another model); with this the device adds the model to `models.json`.
 */
export interface AgentModelInfo {
  /** Display name. */
  name?: string;
  /** Context window in tokens. */
  contextWindow?: number;
  /** Longest answer in tokens. */
  maxOutput?: number;
  /** Whether the model thinks before it answers. */
  reasoning?: boolean;
  /** Whether it reads images. */
  images?: boolean;
  /** US dollars per million tokens. */
  cost?: { input: number; output: number; cacheRead?: number; cacheWrite?: number };
}

/**
 * Messages the server sends to a device. The device checks each against its
 * own configuration (released projects, confirmed chats) before acting.
 */
export type ServerMessage =
  | {
      type: 'welcome';
      /** Protocol version of the server. */
      version: number;
      /** Program version of the server, e.g. `0.1.15`; older devices ignore it. */
      server?: string;
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
      /**
       * Model chosen in the chat; the device maps it to the harness's
       * provider. `info` lets the harness use a model its own catalog lacks
       * (older devices ignore it).
       */
      model: { credentialId: string; modelId: string; info?: AgentModelInfo } | null;
      /** Take the work over after the reply if the check allows (protocol 3). */
      autosync: boolean;
      /** pi's thinking level chosen in the chat; missing for pi's own setting (protocol 4). */
      thinking?: EffortLevel;
      /**
       * Files attached to the message (protocol 9); the device fetches them
       * with its token from `/api/device/files/<id>`.
       */
      files?: ChatFile[];
    }
  | {
      type: 'session.compact';
      /** Session whose agent summarizes its context; the conversation id. */
      session: string;
      /** Project of the chat. */
      project: string;
      /** Harness of the chat. */
      harness: string;
      /** The chat, for the device's log. */
      conversation: { id: string; title: string | null };
    }
  | {
      type: 'session.takeover';
      /** Session whose agent's work to take over; the conversation id. */
      session: string;
      /** Project of the chat. */
      project: string;
      /** Harness of the chat. */
      harness: string;
      /** The chat, for the device's log. */
      conversation: { id: string; title: string | null };
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
    }
  | {
      type: 'settings';
      /** Idle minutes until the device stops its VM; 0: never. Protocol 2. */
      idleMinutes: number;
    }
  | {
      type: 'vm.stop';
      /** Id the device repeats in its `vm.result`. Protocol 2. */
      request: string;
    }
  | {
      type: 'apps.set';
      /** Id the device repeats in its `apps.result`. Protocol 5. */
      request: string;
      /** Project whose apps change. */
      project: string;
      /** Apps it has switched on from now on. */
      enabled: string[];
    }
  | {
      type: 'apps.file';
      /** Id the device repeats in its `apps.result`. Protocol 7. */
      request: string;
      /** A file on the computer; nothing happens before the user approves it there. */
      path: string;
      /** App name it should become. */
      name: string;
      /** Name for people. */
      title: string;
    }
  | {
      type: 'apps.templates';
      /** All own programs of the account, after the hello and on every change. Protocol 6. */
      templates: DeviceAppTemplate[];
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
  /** Take the agent's work over after every reply if the check allows. */
  autosync: boolean;
}

/**
 * Validates a change of a link's settings.
 */
export const updateLinkSchema = z
  .object({
    autosync: z.boolean(),
  })
  .strict();

/** See {@link updateLinkSchema}. */
export type UpdateLinkInput = z.infer<typeof updateLinkSchema>;

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
   * A message of hearth or of the harness itself (warnings, progress). Lines
   * after the first are details the web app shows on demand. With a `code`
   * (an `ApiErrorCode`), the web app shows its own text in the user's
   * language instead.
   */
  notice: { level: 'info' | 'warning' | 'error'; text: string; code?: string };
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
    /**
     * Set in research mode, where only URLs the model did not find ask: the
     * card then says so, and shows the closest found URL if there is one.
     */
    research?: { similar: string | null };
  };
  /** A step on the way to the answer; updated in place. */
  progress: { id: string; label: string; state: 'running' | 'done' | 'failed'; detail?: string };
  /** Taking over the agent's work into the user's folder; updated in place. */
  takeover: TakeoverReport;
  /** Older turns of the chat summarized for the model; updated in place. */
  compaction: Compaction;
  /**
   * What of the agent's work the user's folder lacks, checked on the
   * device after the reply or takeover (protocol 8).
   */
  workspace: { commits: number; changes: number };
};

/**
 * A summary of the older turns of a chat. The model reads it instead of
 * every message up to {@link through}; the chat keeps them all. Decided
 * once and stored, so later requests start the same way and the provider's
 * cache keeps working.
 */
export interface Compaction {
  /** `running` while the summary is written. */
  state: 'running' | 'done' | 'failed';
  /** Automatically because the context ran full, or asked for by the user. */
  trigger: 'auto' | 'manual';
  /** Id of the last message the summary covers. */
  through?: string;
  /** The summary, Markdown; it includes earlier summaries. */
  summary?: string;
  /** Messages covered, earlier summaries included. */
  messages?: number;
  /** Estimated tokens of the covered turns. */
  tokens?: number;
  /** Tokens of the context after the summary; pi's estimate. */
  tokensAfter?: number;
  /** Cost of writing it in US dollars, if the agent reported it. */
  cost?: number;
  /**
   * Written by the agent (pi) of a linked chat for its own context. It has
   * no {@link through}: the chat itself is not shortened for any model.
   */
  agent?: boolean;
  /** Why it failed, for the chat. */
  error?: string;
}

/**
 * Output of a tool call in an agent reply.
 */
export interface AgentToolOutput {
  /** What the tool printed or returned, possibly truncated by the device. */
  text: string;
  /** Places in {@link text} that look like a prompt injection (server's check). */
  findings?: InjectionFinding[];
  /** Jev's opinion on {@link findings}, if asked. */
  assessment?: InjectionAssessment;
}
